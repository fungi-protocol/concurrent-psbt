/// Generate a result-domain product type and its lattice operations from a
/// single field list using standard Rust type syntax.
///
/// Fields are classified automatically:
///
/// - `name: Type`                  → scalar → `name: JoinResult<Type>`
/// - `name: Option<Type>`          → option → `name: Option<JoinResult<Type>>`
/// - `name: BTreeMap<K, V>`        → map    → `name: BTreeMap<K, JoinResult<V>>`
///
/// Classification is syntactic: only the bare identifiers `Option` and
/// `BTreeMap` select the option and map rules. A `macro_rules!` macro cannot
/// resolve paths or aliases, so `std::option::Option<T>`,
/// `std::collections::BTreeMap<K, V>` or a type alias for either is classified
/// as a scalar, which only compiles if the whole type implements
/// `PartialJoin`. Import the types and write them bare.
///
/// Generates:
/// 1. `$Result` struct with transformed field types
/// 2. `$Ext` trait with `wrap` method on the source type
/// 3. `Join for $Result`
/// 4. `$Result::is_ok`
/// 5. `$Result::try_unwrap`
///
/// The field list must name at least one field: `is_ok` is generated as the
/// conjunction of the per-field checks, which has no expansion for an empty
/// list.
macro_rules! joinable_struct {
    // ── Entry point ──────────────────────────────────────────
    (
        $(#[$result_meta:meta])*
        source: $Source:ident,
        result: $Result:ident,
        ext: $Ext:ident,
        fields: { $($fields:tt)* }
    ) => {
        joinable_struct!(@munch
            meta: [ $(#[$result_meta])* ]
            source: $Source
            result: $Result
            ext: $Ext
            classified: []
            rest: [ $($fields)* ]
        );
    };

    // ── Option<T> ────────────────────────────────────────────
    (@munch
        meta: $meta:tt
        source: $Source:ident
        result: $Result:ident
        ext: $Ext:ident
        classified: [ $($classified:tt)* ]
        rest: [ $field:ident : Option < $T:ty > , $($rest:tt)* ]
    ) => {
        joinable_struct!(@munch
            meta: $meta
            source: $Source
            result: $Result
            ext: $Ext
            classified: [ $($classified)* (option $field $T) ]
            rest: [ $($rest)* ]
        );
    };

    // ── BTreeMap<K, V> ──────────────────────────────────────
    (@munch
        meta: $meta:tt
        source: $Source:ident
        result: $Result:ident
        ext: $Ext:ident
        classified: [ $($classified:tt)* ]
        rest: [ $field:ident : BTreeMap < $K:ty , $V:ty > , $($rest:tt)* ]
    ) => {
        joinable_struct!(@munch
            meta: $meta
            source: $Source
            result: $Result
            ext: $Ext
            classified: [ $($classified)* (map $field [ $K => $V ]) ]
            rest: [ $($rest)* ]
        );
    };

    // ── Scalar T (catch-all; must come after Option, BTreeMap) ─
    (@munch
        meta: $meta:tt
        source: $Source:ident
        result: $Result:ident
        ext: $Ext:ident
        classified: [ $($classified:tt)* ]
        rest: [ $field:ident : $T:ty , $($rest:tt)* ]
    ) => {
        joinable_struct!(@munch
            meta: $meta
            source: $Source
            result: $Result
            ext: $Ext
            classified: [ $($classified)* (scalar $field $T) ]
            rest: [ $($rest)* ]
        );
    };

    // ── Terminal: no more fields ─────────────────────────────
    (@munch
        meta: [ $(#[$result_meta:meta])* ]
        source: $Source:ident
        result: $Result:ident
        ext: $Ext:ident
        classified: [ $( ($category:ident $field:ident $type_tokens:tt) )* ]
        rest: []
    ) => {
        // 1. Result struct
        $(#[$result_meta])*
        #[derive(Debug, Clone, PartialEq)]
        pub struct $Result {
            $(
                pub(crate) $field: joinable_struct!(@result_type $category $type_tokens),
            )*
        }

        // 2. Ext trait + impl
        pub trait $Ext {
            fn wrap(self) -> $Result;
        }

        impl $Ext for $Source {
            fn wrap(self) -> $Result {
                $Result {
                    $(
                        $field: joinable_struct!(@wrap $category self . $field),
                    )*
                }
            }
        }

        // 3. Join
        impl $crate::lattice::join::Join for $Result {
            fn join(self, other: Self) -> Self {
                $Result {
                    $(
                        $field: self.$field.join(other.$field),
                    )*
                }
            }
        }

        // 4. is_ok + 5. try_unwrap
        impl $Result {
            pub fn is_ok(&self) -> bool {
                $(
                    joinable_struct!(@is_ok $category self . $field)
                )&&*
            }

            /// Visit each conflicted field, calling `f(field_name, key, &conflict)`.
            ///
            /// Only visits fields that contain conflicts. Clean fields are skipped.
            /// For map fields, visits each conflicted entry with `Some(&key)`, so
            /// callers can tell which entry conflicts. For scalar and option
            /// fields `key` is `None`.
            #[cfg_attr(
                not(test),
                expect(dead_code, reason = "only tests enumerate conflicted fields so far")
            )]
            pub(crate) fn for_each_conflict(
                &self,
                mut f: impl FnMut(&str, Option<&dyn std::fmt::Debug>, &dyn std::fmt::Debug),
            ) {
                $(
                    joinable_struct!(@visit_conflict $category self . $field, f);
                )*
            }

            pub fn try_unwrap(self) -> Result<$Source, Self> {
                if !self.is_ok() {
                    return Err(self);
                }
                Ok($Source {
                    $(
                        $field: joinable_struct!(@unwrap $category self . $field),
                    )*
                })
            }
        }
    };

    // --- Type transformation rules ---

    (@result_type scalar $T:ty) => {
        $crate::lattice::partial::JoinResult<$T>
    };
    (@result_type option $T:ty) => {
        Option<$crate::lattice::partial::JoinResult<$T>>
    };
    (@result_type map [$K:ty => $V:ty]) => {
        std::collections::BTreeMap<$K, $crate::lattice::partial::JoinResult<$V>>
    };

    // --- Wrap rules ---

    (@wrap scalar $self:ident . $field:ident) => {
        $crate::lattice::partial::PartialJoin::wrap($self.$field)
    };
    (@wrap option $self:ident . $field:ident) => {
        $crate::collections::option::OptionExt::wrap($self.$field)
    };
    (@wrap map $self:ident . $field:ident) => {
        $crate::collections::btreemap::BTreeMapExt::wrap($self.$field)
    };

    // --- is_ok rules ---

    (@is_ok scalar $self:ident . $field:ident) => {
        $self.$field.is_ok()
    };
    (@is_ok option $self:ident . $field:ident) => {
        $crate::collections::option::ResultOptionExt::is_ok(&$self.$field)
    };
    (@is_ok map $self:ident . $field:ident) => {
        $crate::collections::btreemap::ResultBTreeMapExt::is_ok(&$self.$field)
    };

    // --- Visit-conflict rules ---

    (@visit_conflict scalar $self:ident . $field:ident , $f:expr) => {
        if let Err(c) = &$self.$field {
            ($f)(stringify!($field), None, c as &dyn std::fmt::Debug);
        }
    };
    (@visit_conflict option $self:ident . $field:ident , $f:expr) => {
        if let Some(Err(c)) = &$self.$field {
            ($f)(stringify!($field), None, c as &dyn std::fmt::Debug);
        }
    };
    (@visit_conflict map $self:ident . $field:ident , $f:expr) => {
        for (k, v) in &$self.$field {
            if let Err(c) = v {
                ($f)(
                    stringify!($field),
                    Some(k as &dyn std::fmt::Debug),
                    c as &dyn std::fmt::Debug,
                );
            }
        }
    };

    // --- Unwrap rules ---

    (@unwrap scalar $self:ident . $field:ident) => {
        $self.$field.expect("verified all fields are Ok")
    };
    (@unwrap option $self:ident . $field:ident) => {
        $crate::collections::option::ResultOptionExt::try_unwrap($self.$field)
            .expect("verified all fields are Ok")
    };
    (@unwrap map $self:ident . $field:ident) => {
        $crate::collections::btreemap::ResultBTreeMapExt::try_unwrap($self.$field)
            .expect("verified all fields are Ok")
    };
}

#[cfg(all(test, feature = "unit-tests"))]
#[cfg_attr(coverage_nightly, coverage(off))]
mod tests {
    use std::collections::BTreeMap;

    use crate::lattice::join::Join;

    #[derive(Debug, PartialEq)]
    pub struct Sample {
        scalar: u8,
        option: Option<u8>,
        map: BTreeMap<u8, u8>,
    }

    joinable_struct! {
        /// Result-domain form of [`Sample`], one field per category.
        source: Sample,
        result: ResultSample,
        ext: SampleExt,
        fields: {
            scalar: u8,
            option: Option<u8>,
            map: BTreeMap<u8, u8>,
        }
    }

    fn sample(scalar: u8, option: u8, value: u8) -> Sample {
        Sample {
            scalar,
            option: Some(option),
            map: BTreeMap::from([(0, value)]),
        }
    }

    #[test]
    fn equal_structs_join_and_unwrap() {
        let joined = sample(1, 2, 3).wrap().join(sample(1, 2, 3).wrap());
        assert!(joined.is_ok());
        assert_eq!(joined.try_unwrap().ok(), Some(sample(1, 2, 3)));
    }

    #[test]
    fn conflicting_fields_are_reported_by_name_and_key() {
        let joined = sample(1, 2, 3).wrap().join(sample(4, 5, 6).wrap());
        assert!(!joined.is_ok());
        let mut fields = Vec::new();
        joined.for_each_conflict(|field, key, _| {
            fields.push((field.to_owned(), key.map(|k| format!("{k:?}"))));
        });
        assert_eq!(
            fields,
            [
                ("scalar".to_owned(), None),
                ("option".to_owned(), None),
                ("map".to_owned(), Some("0".to_owned())),
            ]
        );
        assert!(joined.try_unwrap().is_err());
    }
}
