/// Generate a product type, its result-domain counterpart, and their lattice
/// operations from a single field list using standard Rust type syntax.
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
/// Each field may carry attributes and a visibility, which apply to the
/// `$Source` field; fields of `$Result` are `pub(crate)`.
///
/// Generates:
/// 1. `$Source` struct with the fields as written
/// 2. `$Result` struct with transformed field types
/// 3. `$Source::wrap`
/// 4. `Join for $Result`
/// 5. `$Result::is_ok`
/// 6. `$Result::try_unwrap`
///
/// The field list must name at least one field: `is_ok` is generated as the
/// conjunction of the per-field checks, which has no expansion for an empty
/// list.
macro_rules! joinable_struct {
    // ── Entry point ──────────────────────────────────────────
    (
        $(#[$source_meta:meta])*
        source: $Source:ident,
        $(#[$result_meta:meta])*
        result: $Result:ident,
        fields: { $($fields:tt)* }
    ) => {
        joinable_struct!(@munch
            source_meta: [ $(#[$source_meta])* ]
            result_meta: [ $(#[$result_meta])* ]
            source: $Source
            result: $Result
            classified: []
            rest: [ $($fields)* ]
        );
    };

    // ── Option<T> ────────────────────────────────────────────
    (@munch
        source_meta: $source_meta:tt
        result_meta: $result_meta:tt
        source: $Source:ident
        result: $Result:ident
        classified: [ $($classified:tt)* ]
        rest: [
            $(#[$field_meta:meta])* $vis:vis $field:ident : Option < $T:ty > ,
            $($rest:tt)*
        ]
    ) => {
        joinable_struct!(@munch
            source_meta: $source_meta
            result_meta: $result_meta
            source: $Source
            result: $Result
            classified: [
                $($classified)*
                (option [ $(#[$field_meta])* ] [ $vis ] $field $T)
            ]
            rest: [ $($rest)* ]
        );
    };

    // ── BTreeMap<K, V> ──────────────────────────────────────
    (@munch
        source_meta: $source_meta:tt
        result_meta: $result_meta:tt
        source: $Source:ident
        result: $Result:ident
        classified: [ $($classified:tt)* ]
        rest: [
            $(#[$field_meta:meta])* $vis:vis $field:ident : BTreeMap < $K:ty , $V:ty > ,
            $($rest:tt)*
        ]
    ) => {
        joinable_struct!(@munch
            source_meta: $source_meta
            result_meta: $result_meta
            source: $Source
            result: $Result
            classified: [
                $($classified)*
                (map [ $(#[$field_meta])* ] [ $vis ] $field [ $K => $V ])
            ]
            rest: [ $($rest)* ]
        );
    };

    // ── Scalar T (catch-all; must come after Option, BTreeMap) ─
    (@munch
        source_meta: $source_meta:tt
        result_meta: $result_meta:tt
        source: $Source:ident
        result: $Result:ident
        classified: [ $($classified:tt)* ]
        rest: [
            $(#[$field_meta:meta])* $vis:vis $field:ident : $T:ty ,
            $($rest:tt)*
        ]
    ) => {
        joinable_struct!(@munch
            source_meta: $source_meta
            result_meta: $result_meta
            source: $Source
            result: $Result
            classified: [
                $($classified)*
                (scalar [ $(#[$field_meta])* ] [ $vis ] $field $T)
            ]
            rest: [ $($rest)* ]
        );
    };

    // ── Terminal: no more fields ─────────────────────────────
    (@munch
        source_meta: [ $(#[$source_meta:meta])* ]
        result_meta: [ $(#[$result_meta:meta])* ]
        source: $Source:ident
        result: $Result:ident
        classified: [
            $( (
                $category:ident
                [ $(#[$field_meta:meta])* ]
                [ $vis:vis ]
                $field:ident
                $type_tokens:tt
            ) )*
        ]
        rest: []
    ) => {
        // 1. Source struct
        $(#[$source_meta])*
        #[derive(Debug, Clone, PartialEq, Eq)]
        pub struct $Source {
            $(
                $(#[$field_meta])*
                $vis $field: joinable_struct!(@source_type $category $type_tokens),
            )*
        }

        // 2. Result struct
        $(#[$result_meta])*
        #[derive(Debug, Clone, PartialEq)]
        pub struct $Result {
            $(
                pub(crate) $field: joinable_struct!(@result_type $category $type_tokens),
            )*
        }

        // 3. wrap
        impl $Source {
            #[doc = concat!(
                "Lift into the result domain as a conflict-free [`",
                stringify!($Result),
                "`]."
            )]
            pub fn wrap(self) -> $Result {
                $Result {
                    $(
                        $field: joinable_struct!(@wrap $category self . $field),
                    )*
                }
            }
        }

        // 4. Join
        impl $crate::lattice::join::Join for $Result {
            fn join(self, other: Self) -> Self {
                $Result {
                    $(
                        $field: self.$field.join(other.$field),
                    )*
                }
            }
        }

        // 5. is_ok + 6. try_unwrap
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

    // --- Type rules ---

    (@source_type scalar $T:ty) => {
        $T
    };
    (@source_type option $T:ty) => {
        Option<$T>
    };
    (@source_type map [$K:ty => $V:ty]) => {
        std::collections::BTreeMap<$K, $V>
    };

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

    joinable_struct! {
        /// One field per category.
        source: Sample,
        /// Result-domain form of [`Sample`].
        result: ResultSample,
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
