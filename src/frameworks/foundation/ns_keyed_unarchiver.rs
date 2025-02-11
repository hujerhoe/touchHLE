/*
 * This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at https://mozilla.org/MPL/2.0/.
 */
//! `NSKeyedUnarchiver` and deserialization of its object graph format.
//!
//! Resources:
//! - You can get a good intuitive grasp of how the format works just by staring
//!   at a pretty-print of a simple nib file from something that can parse
//!   plists, e.g. `plutil -p` or `println!("{:#?}", plist::Value::...);`.
//! - Apple's [Archives and Serializations Programming Guide](https://developer.apple.com/library/archive/documentation/Cocoa/Conceptual/Archiving/Articles/archives.html)

use super::ns_string::{from_rust_string, get_static_str, to_rust_string};
use crate::dyld::{ConstantExports, HostConstant};
use crate::frameworks::core_graphics::{CGPoint, CGRect, CGSize};
use crate::frameworks::foundation::{NSInteger, NSUInteger};
use crate::frameworks::uikit::ui_geometry::{
    CGPointFromString, CGRectFromString, CGSizeFromString,
};
use crate::mem::{ConstVoidPtr, GuestUSize, MutVoidPtr};
use crate::objc::{
    autorelease, id, msg, msg_class, nil, objc_classes, release, retain, ClassExports, HostObject,
    NSZonePtr,
};
use crate::Environment;
use plist::{Dictionary, Uid, Value};
use std::io::Cursor;

pub const NSKeyedArchiveRootObjectKey: &str = "root";

pub const CONSTANTS: ConstantExports = &[(
    "_NSKeyedArchiveRootObjectKey",
    HostConstant::NSString(NSKeyedArchiveRootObjectKey),
)];

struct NSKeyedUnarchiverHostObject {
    plist: Dictionary,
    current_key: Option<Uid>,
    /// linear map of Uid => id
    already_unarchived: Vec<Option<id>>,
    /// Something responding to NSKeyedUnarchiverDelegate
    delegate: id,
}
impl HostObject for NSKeyedUnarchiverHostObject {}

pub const CLASSES: ClassExports = objc_classes! {

(env, this, _cmd);

@implementation NSKeyedUnarchiver: NSCoder

+ (id)allocWithZone:(NSZonePtr)_zone { // struct _NSZone*
    let unarchiver = Box::new(NSKeyedUnarchiverHostObject {
        plist: Dictionary::new(),
        current_key: None,
        already_unarchived: Vec::new(),
        delegate: nil
    });
    env.objc.alloc_object(this, unarchiver, &mut env.mem)
}

+ (id)unarchiveObjectWithFile:(id)path { // NSString *
    let data: id = msg_class![env; NSData dataWithContentsOfFile:path];
    if data == nil {
        return nil;
    }
    msg![env; this unarchiveObjectWithData:data]
}

+ (id)unarchiveObjectWithData:(id)data { // NSData *
    let new: id = msg![env; this alloc];
    let new: id = msg![env; new initForReadingWithData:data];
    let root_key = get_static_str(env, NSKeyedArchiveRootObjectKey);
    let result: id = msg![env; new decodeObjectForKey:root_key];
    autorelease(env, result)
}

// TODO: other init methods.

- (id)initForReadingWithData:(id)data { // NSData *
    if data == nil {
        return nil;
    }

    let length: NSUInteger = msg![env; data length];
    let bytes: ConstVoidPtr = msg![env; data bytes];
    let slice = env.mem.bytes_at(bytes.cast(), length);

    let host_obj = env.objc.borrow_mut::<NSKeyedUnarchiverHostObject>(this);
    assert!(host_obj.already_unarchived.is_empty());
    assert!(host_obj.current_key.is_none());
    assert!(host_obj.plist.is_empty());

    let plist = Value::from_reader(Cursor::new(slice)).unwrap();
    let plist = plist.into_dictionary().unwrap();
    assert!(plist["$version"].as_unsigned_integer() == Some(100000));
    assert!(plist["$archiver"].as_string() == Some("NSKeyedArchiver"));

    let key_count = plist["$objects"].as_array().unwrap().len();

    host_obj.already_unarchived = vec![None; key_count];
    host_obj.plist = plist;

    this
}

- (())dealloc {
    let host_obj = borrow_host_obj(env, this);
    let already_unarchived = std::mem::take(&mut host_obj.already_unarchived);

    for &object in already_unarchived.iter().flatten() {
        release(env, object);
    }

    env.objc.dealloc_object(this, &mut env.mem)
}

// TODO: implement calls to delegate methods
// weak/non-retaining
- (())setDelegate:(id)delegate { // id<NSKeyedUnarchiverDelegate>
    let host_object = env.objc.borrow_mut::<NSKeyedUnarchiverHostObject>(this);
    host_object.delegate = delegate;
}
- (id)delegate {
    env.objc.borrow::<NSKeyedUnarchiverHostObject>(this).delegate
}

// These methods drive most of the decoding. They get called in two cases:
// - By the code that initiates the unarchival, e.g. UINib, to retrieve
//   top-level objects.
// - By the object currently being unarchived, i.e. something that had
//   `initWithCoder:` called on it, to retrieve objects from its scope.
// They are all from the NSCoder abstract class and they return default values
// if the key is unknown.

- (bool)decodeBoolForKey:(id)key { // NSString *
    get_value_to_decode_for_key(env, this, key)
        .is_some_and(|value| value.as_boolean().unwrap())
}

- (f64)decodeDoubleForKey:(id)key { // NSString *
    get_value_to_decode_for_key(env, this, key).map_or(
        0.0,
        |value| value.as_real().unwrap()
    )
}

- (f32)decodeFloatForKey:(id)key { // NSString *
    // TODO: Check bounds, raise NSRangeException if it doesn't fit
    get_value_to_decode_for_key(env, this, key).map_or(
        0.0,
        |value| value.as_real().unwrap()
    ) as f32
}

- (NSInteger)decodeIntegerForKey:(id)key { // NSString *
    // TODO: Check bounds, raise NSRangeException if it doesn't fit
    get_value_to_decode_for_key(env, this, key).map_or(
        0,
        |value| value.as_signed_integer().unwrap()
    ).try_into().unwrap()
}

- (i32)decodeIntForKey:(id)key { // NSString *
    // TODO: Check bounds, raise NSRangeException if it doesn't fit
    get_value_to_decode_for_key(env, this, key).map_or(
        0,
        |value| value.as_signed_integer().unwrap()
    ).try_into().unwrap()
}

- (i32)decodeInt32ForKey:(id)key { // NSString *
    // TODO: Check bounds, raise NSRangeException if it doesn't fit
    get_value_to_decode_for_key(env, this, key).map_or(
        0,
        |value| value.as_signed_integer().unwrap()
    ).try_into().unwrap()
}

- (i64)decodeInt64ForKey:(id)key { // NSString *
    get_value_to_decode_for_key(env, this, key).map_or(
        0,
        |value| value.as_signed_integer().unwrap()
    )
}

- (id)decodeObjectForKey:(id)key { // NSString*
    let Some(next_uid) = get_value_to_decode_for_key(env, this, key) else {
        return nil;
    };
    let next_uid = next_uid.as_uid().copied().unwrap();
    let object = unarchive_key(env, this, next_uid);

    // on behalf of the caller
    retain(env, object);
    autorelease(env, object)
}

- (bool)containsValueForKey:(id)key { // NSString*
    assert!(key != nil);
    get_value_to_decode_for_key(env, this, key).is_some()
}

// TODO: add more decode methods

// These come from a category in UIKit's UIGeometry.h
- (CGPoint)decodeCGPointForKey:(id)key { // NSString*
    let string: id = msg![env; this decodeObjectForKey:key];
    CGPointFromString(env, string)
}
- (CGSize)decodeCGSizeForKey:(id)key { // NSString*
    let string: id = msg![env; this decodeObjectForKey:key];
    CGSizeFromString(env, string)
}
- (CGRect)decodeCGRectForKey:(id)key { // NSString*
    let string: id = msg![env; this decodeObjectForKey:key];
    CGRectFromString(env, string)
}

@end

};

fn borrow_host_obj(env: &mut Environment, unarchiver: id) -> &mut NSKeyedUnarchiverHostObject {
    env.objc.borrow_mut(unarchiver)
}

fn get_value_to_decode_for_key(env: &mut Environment, unarchiver: id, key: id) -> Option<&Value> {
    let key = to_rust_string(env, key); // TODO: avoid copying string
    let host_obj = borrow_host_obj(env, unarchiver);
    let scope = match host_obj.current_key {
        Some(current_uid) => {
            &host_obj.plist["$objects"].as_array().unwrap()[current_uid.get() as usize]
        }
        None => &host_obj.plist["$top"],
    }
    .as_dictionary()
    .unwrap();
    scope.get(&key)
}

/// The core of the implementation: unarchive something by its uid.
///
/// This is recursive in practice: the `initWithCoder:` messages sent by this
/// function will be received by objects which will then send
/// `decodeXXXWithKey:` messages back to the unarchiver, which will then call
/// this function (and so on).
///
/// The object returned is retained only by the archiver. Remember to retain and
/// possibly autorelease it as appropriate.
fn unarchive_key(env: &mut Environment, unarchiver: id, key: Uid) -> id {
    let host_obj = borrow_host_obj(env, unarchiver);
    if let Some(existing) = host_obj.already_unarchived[key.get() as usize] {
        return existing;
    }

    let objects = host_obj.plist["$objects"].as_array().unwrap();

    let item = &objects[key.get() as usize];
    let new_object = match item {
        // The most general kind of item: a dictionary that contains the info
        // needed to invoke `initWithCoder:` on a class implementing NSCoding.
        Value::Dictionary(dict) => {
            let class_key = dict["$class"].as_uid().copied().unwrap();
            let class;
            if let Some(existing) = host_obj.already_unarchived[class_key.get() as usize] {
                class = existing;
            } else {
                let class_dict = &objects[class_key.get() as usize];
                let class_dict = class_dict.as_dictionary().unwrap();

                let class_name = class_dict["$classname"].as_string().unwrap();

                class = {
                    // get_known_class needs &mut ObjC, so we can't call it
                    // while holding a reference to the class name, since it
                    // is ultimately owned by ObjC via the host object
                    let class_name = class_name.to_string();
                    env.objc.get_known_class(&class_name, &mut env.mem)
                };
                let host_obj = borrow_host_obj(env, unarchiver); // reborrow

                host_obj.already_unarchived[class_key.get() as usize] = Some(class);
            };

            let host_obj = borrow_host_obj(env, unarchiver); // reborrow
            let old_current_key = host_obj.current_key;
            host_obj.current_key = Some(key);

            let new_object: id = msg![env; class alloc];
            let new_object: id = msg![env; new_object initWithCoder:unarchiver];

            let host_obj = borrow_host_obj(env, unarchiver); // reborrow
            host_obj.current_key = old_current_key;

            new_object
        }
        Value::String(s) => {
            let s = s.to_string();
            from_rust_string(env, s)
        }
        Value::Integer(int) => {
            #[allow(clippy::clone_on_copy)]
            let int = int.clone();
            // Similar logic to deserialize_plist()
            let number: id = msg_class![env; NSNumber alloc];
            // TODO: is this the correct order of preference? does it matter?
            if let Some(int64) = int.as_signed() {
                let longlong: i64 = int64;
                msg![env; number initWithLongLong:longlong]
            } else if let Some(uint64) = int.as_unsigned() {
                let ulonglong: u64 = uint64;
                msg![env; number initWithUnsignedLongLong:ulonglong]
            } else {
                unreachable!(); // according to plist crate docs
            }
        }
        _ => unimplemented!("Unarchive: {:#?}", item),
    };

    let host_obj = borrow_host_obj(env, unarchiver); // reborrow
    host_obj.already_unarchived[key.get() as usize] = Some(new_object);
    new_object
}

/// Shortcut for use by `[_touchHLE_NSArray initWithCoder:]`.
///
/// The objects are to be considered retained by the `Vec`.
pub fn decode_current_array(env: &mut Environment, unarchiver: id) -> Vec<id> {
    let keys = keys_for_key(env, unarchiver, "NS.objects");

    keys.into_iter()
        .map(|key| {
            let new_object = unarchive_key(env, unarchiver, key);
            // object is retained by the Vec
            retain(env, new_object)
        })
        .collect()
}

/// Shortcut for use by `[_touchHLE_NSMutableDictionary initWithCoder:]`.
///
/// Similar to `decode_current_array`, but for dictionaries.
/// The keys and objects are not retained!
pub fn decode_current_dict(env: &mut Environment, unarchiver: id) -> Vec<(id, id)> {
    let keys = keys_for_key(env, unarchiver, "NS.keys");
    let vals = keys_for_key(env, unarchiver, "NS.objects");
    log_dbg!("decode_current_dict: keys {:?}, vals {:?}", keys, vals);

    let keys: Vec<id> = keys
        .into_iter()
        .map(|key| unarchive_key(env, unarchiver, key))
        .collect();
    let vals: Vec<id> = vals
        .into_iter()
        .map(|val| unarchive_key(env, unarchiver, val))
        .collect();

    keys.into_iter().zip(vals).collect()
}

/// Shortcut for use by `[NSDate initWithCoder:]`.
pub fn decode_current_date(env: &mut Environment, unarchiver: id) -> id {
    let key = get_static_str(env, "NS.time");
    let timestamp = get_value_to_decode_for_key(env, unarchiver, key)
        .unwrap()
        .as_real()
        .unwrap();

    let date: id = msg_class![env; NSDate alloc];
    msg![env; date initWithTimeIntervalSinceReferenceDate:timestamp]
}

/// Shortcut for use by `[NSData initWithCoder:]`.
pub fn decode_current_data(env: &mut Environment, unarchiver: id, is_mutable: bool) -> id {
    let key = get_static_str(env, "NS.data");
    // TODO: avoid copying (twice!)
    let bytes = get_value_to_decode_for_key(env, unarchiver, key)
        .unwrap()
        .as_data()
        .unwrap()
        .to_vec();
    let len: GuestUSize = bytes.len().try_into().unwrap();
    let guest_bytes: MutVoidPtr = env.mem.alloc(len);
    env.mem
        .bytes_at_mut(guest_bytes.cast(), len)
        .copy_from_slice(bytes.as_slice());

    assert!(is_mutable); // TODO
    let data: id = msg_class![env; NSMutableData alloc];
    msg![env; data initWithBytesNoCopy:guest_bytes length:len freeWhenDone:true]
}

fn keys_for_key(env: &mut Environment, unarchiver: id, key: &str) -> Vec<Uid> {
    let host_obj = borrow_host_obj(env, unarchiver);
    let objects = host_obj.plist["$objects"].as_array().unwrap();
    let item = &objects[host_obj.current_key.unwrap().get() as usize];
    let keys = item.as_dictionary().unwrap()[key].as_array().unwrap();
    keys.iter()
        .map(|value| value.as_uid().copied().unwrap())
        .collect()
}
