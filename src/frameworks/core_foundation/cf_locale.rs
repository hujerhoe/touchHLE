/*
 * This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at https://mozilla.org/MPL/2.0/.
 */
//! `CFLocale`

use super::cf_allocator::CFAllocatorRef;
use super::cf_array::CFArrayRef;
use super::cf_string::CFStringRef;
use super::CFTypeRef;
use crate::dyld::{export_c_func, ConstantExports, FunctionExports, HostConstant};
use crate::frameworks::foundation::NSUInteger;
use crate::objc::{id, msg, msg_class};
use crate::Environment;

type CFLocaleIdentifier = CFStringRef;
/// `NSLocale` is toll-free bridged with `CFLocaleRef`
type CFLocaleRef = CFTypeRef;
type CFLocaleKey = CFStringRef;

pub const kCFLocaleCountryCode: &str = "kCFLocaleCountryCodeKey";

pub const CONSTANTS: ConstantExports = &[(
    "_kCFLocaleCountryCode",
    HostConstant::NSString(kCFLocaleCountryCode),
)];

fn CFLocaleCopyCurrent(env: &mut Environment) -> CFLocaleRef {
    let locale: id = msg_class![env; NSLocale currentLocale];
    msg![env; locale copy]
}

fn CFLocaleCopyPreferredLanguages(env: &mut Environment) -> CFArrayRef {
    let arr = msg_class![env; NSLocale preferredLanguages];
    msg![env; arr copy]
}

fn CFLocaleCreateCanonicalLocaleIdentifierFromString(
    env: &mut Environment,
    allocator: CFAllocatorRef,
    locale_identifier: CFStringRef,
) -> CFLocaleIdentifier {
    assert!(allocator.is_null());
    let len: NSUInteger = msg![env; locale_identifier length];
    // TODO: support arbitrary locale identification strings
    assert_eq!(len, 2);
    let ns_string: id = msg_class![env; NSString alloc];
    msg![env; ns_string initWithString:locale_identifier]
}

fn CFLocaleGetSystem(env: &mut Environment) -> CFLocaleRef {
    msg_class![env; NSLocale systemLocale]
}

fn CFLocaleGetValue(env: &mut Environment, locale: CFLocaleRef, key: CFLocaleKey) -> CFTypeRef {
    msg![env; locale objectForKey:key]
}

pub const FUNCTIONS: FunctionExports = &[
    export_c_func!(CFLocaleCopyCurrent()),
    export_c_func!(CFLocaleCopyPreferredLanguages()),
    export_c_func!(CFLocaleCreateCanonicalLocaleIdentifierFromString(_, _)),
    export_c_func!(CFLocaleGetSystem()),
    export_c_func!(CFLocaleGetValue(_, _)),
];
