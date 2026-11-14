/// Converts a mirrored field whose type differs from the psbt-v2 field of the
/// same name.
pub(crate) trait MirroredField {
    /// The psbt-v2 field's type.
    type V2;
    /// The field's type.
    type Value;
    /// Convert from the psbt-v2 field.
    fn from_v2(value: Self::V2) -> Self::Value;
    /// Convert into the psbt-v2 field.
    fn into_v2(value: Self::Value) -> Self::V2;
}

/// Generate a product type, its result-domain counterpart, and their lattice
/// operations from a single field list using standard Rust type syntax.
///
/// Each field's result-domain type is `<Type as Lift>::Result`, so a field
/// joins the way its type [`Lift`](crate::lattice::lift::Lift)s: a
/// [`PartialJoin`](crate::PartialJoin) scalar as a
/// [`JoinResult`](crate::JoinResult), and `Option` and `BTreeMap` by lifting
/// their contents. Paths and type aliases resolve like any other type.
///
/// Each field may carry attributes and a visibility, which apply to the
/// `$Source` field; fields of `$Result` are `pub(crate)`.
///
/// Generates:
/// 1. `$Source` struct with the fields as written
/// 2. `$Result` struct with the lifted field types
/// 3. `$Source::wrap`
/// 4. `Join for $Result`
/// 5. `$Result::is_ok`, `$Result::for_each_conflict` and `$Result::try_unwrap`
/// 6. `ResultValue for $Result`
///
/// Each entry in `fields` mirrors the field of the same name in the `v2` type.
/// If the two types differ, an `=` after the type names the [`MirroredField`]
/// that converts between them. Each entry in `proprietary` is written
/// `name: Type = Codec`, and is stored in a proprietary key of `v2` by the
/// [`ProprietaryField`](crate::psbt::proprietary::ProprietaryField) impl
/// `Codec`. The struct then has private conversions:
///
/// - `from_v2_fields(v2, proprietary...)` moves the mirrored fields out of
///   `v2` and takes the proprietary fields as given;
/// - `decode_v2(v2)` decodes the proprietary fields in order, removing their
///   keys, then moves the mirrored fields;
/// - `encode_v2(self, base)` moves the mirrored fields into `base` and
///   encodes the proprietary fields into its proprietary keys. Fields of
///   `base` that are not mirrored are kept.
macro_rules! joinable_struct {
    (
        $(#[$source_meta:meta])*
        source: $Source:ident,
        $(#[$result_meta:meta])*
        result: $Result:ident,
        v2: $V2:path,
        fields: {
            $(
                $(#[$field_meta:meta])* $vis:vis
                $field:ident : $T:ty $( = $Conversion:ty )?
            ),+ $(,)?
        },
        proprietary: {
            $(
                $(#[$proprietary_meta:meta])* $proprietary_vis:vis
                $proprietary:ident : $P:ty = $Codec:ty
            ),* $(,)?
        } $(,)?
    ) => {
        joinable_struct! {
            $(#[$source_meta])*
            source: $Source,
            $(#[$result_meta])*
            result: $Result,
            fields: {
                $( $(#[$field_meta])* $vis $field: $T, )+
                $( $(#[$proprietary_meta])* $proprietary_vis $proprietary: $P, )*
            }
        }

        impl $Source {
            /// The mirrored fields of `v2`, with these proprietary fields.
            fn from_v2_fields(v2: $V2, $( $proprietary: $P ),*) -> Self {
                Self {
                    $( $field: joinable_struct!(@from_v2 v2.$field $(, $Conversion)?), )+
                    $( $proprietary, )*
                }
            }

            /// Decode the proprietary fields of `v2`, removing their keys, and
            /// move its mirrored fields.
            ///
            /// # Errors
            /// Returns the error of the first proprietary field that does not
            /// decode.
            fn decode_v2<E>(v2: $V2) -> Result<Self, E>
            where
                $( E: From<<$Codec as $crate::psbt::proprietary::ProprietaryField>::Error>, )*
            {
                joinable_struct!(@decode v2 [ $( $proprietary $Codec ),* ])
            }

            /// Move the mirrored fields into `base`, and encode the proprietary
            /// fields into its proprietary keys.
            ///
            /// If a proprietary field encodes to `None`, its key is removed from
            /// `base`, even if `self.proprietaries` has it.
            fn encode_v2(self, mut base: $V2) -> $V2 {
                $( base.$field = joinable_struct!(@into_v2 self.$field $(, $Conversion)?); )+
                $(
                    let key = $crate::psbt::proprietary::key(
                        <$Codec as $crate::psbt::proprietary::ProprietaryField>::SUBTYPE,
                    );
                    base.proprietaries.remove(&key);
                    base.proprietaries.extend(
                        <$Codec as $crate::psbt::proprietary::ProprietaryField>::encode(
                            self.$proprietary,
                        )
                        .map(|value| (key, value)),
                    );
                )*
                base
            }
        }
    };

    (@from_v2 $value:expr) => {
        $value
    };

    (@from_v2 $value:expr, $Conversion:ty) => {
        <$Conversion as $crate::psbt::joinable_struct_macro::MirroredField>::from_v2($value)
    };

    (@into_v2 $value:expr) => {
        $value
    };

    (@into_v2 $value:expr, $Conversion:ty) => {
        <$Conversion as $crate::psbt::joinable_struct_macro::MirroredField>::into_v2($value)
    };

    (@decode $v2:ident []) => {
        Ok(Self::from_v2_fields($v2))
    };

    (@decode $v2:ident [ $( $proprietary:ident $Codec:ty ),+ ]) => {{
        let mut $v2 = $v2;
        $(
            let $proprietary =
                <$Codec as $crate::psbt::proprietary::ProprietaryField>::decode(
                    $v2.proprietaries.remove(&$crate::psbt::proprietary::key(
                        <$Codec as $crate::psbt::proprietary::ProprietaryField>::SUBTYPE,
                    )),
                )?;
        )+
        Ok(Self::from_v2_fields($v2, $( $proprietary ),+))
    }};

    (
        $(#[$source_meta:meta])*
        source: $Source:ident,
        $(#[$result_meta:meta])*
        result: $Result:ident,
        fields: {
            $( $(#[$field_meta:meta])* $vis:vis $field:ident : $T:ty ),+ $(,)?
        }
    ) => {
        $(#[$source_meta])*
        #[derive(Debug, Clone, PartialEq, Eq)]
        pub struct $Source {
            $(
                $(#[$field_meta])*
                $vis $field: $T,
            )+
        }

        $(#[$result_meta])*
        #[derive(Debug, Clone, PartialEq)]
        pub struct $Result {
            $(
                pub(crate) $field: <$T as $crate::lattice::lift::Lift>::Result,
            )+
        }

        impl $Source {
            #[doc = concat!(
                "Lift into the result domain as a conflict-free [`",
                stringify!($Result),
                "`]."
            )]
            pub fn wrap(self) -> $Result {
                $Result {
                    $(
                        $field: $crate::lattice::lift::Lift::lift(self.$field),
                    )+
                }
            }
        }

        impl $crate::lattice::join::Join for $Result {
            fn join(self, other: Self) -> Self {
                $Result {
                    $(
                        $field: self.$field.join(other.$field),
                    )+
                }
            }
        }

        impl $Result {
            /// Returns `true` if no field holds a conflict.
            pub fn is_ok(&self) -> bool {
                $(
                    $crate::lattice::lift::ResultValue::is_ok(&self.$field)
                )&&+
            }

            /// Visit each conflicted field, calling `f(field_name, key, &conflict)`.
            ///
            /// Only visits fields that contain conflicts. Clean fields are skipped.
            /// For map fields and options holding maps, visits each conflicted
            /// entry with `Some(&key)`, so callers can tell which entry conflicts. For other
            /// fields `key` is `None`.
            #[cfg_attr(
                not(all(test, feature = "unit-tests")),
                expect(dead_code, reason = "only unit tests enumerate conflicted fields until #44 settles conflict iteration")
            )]
            pub(crate) fn for_each_conflict(
                &self,
                mut f: impl FnMut(&str, Option<&dyn std::fmt::Debug>, &dyn std::fmt::Debug),
            ) {
                $(
                    $crate::lattice::lift::VisitConflicts::visit_conflicts(
                        &self.$field,
                        &mut |key, conflict| f(stringify!($field), key, conflict),
                    );
                )+
            }

            /// Extract the clean value, or return `self` if any field holds a
            /// conflict.
            pub fn try_unwrap(self) -> Result<$Source, Self> {
                if !self.is_ok() {
                    return Err(self);
                }
                Ok($Source {
                    $(
                        $field: $crate::lattice::lift::ResultValue::try_unwrap(self.$field)
                            .expect("verified all fields are Ok"),
                    )+
                })
            }
        }

        impl $crate::lattice::lift::ResultValue for $Result {
            type Clean = $Source;

            fn is_ok(&self) -> bool {
                $Result::is_ok(self)
            }

            fn try_unwrap(self) -> Result<$Source, Self> {
                $Result::try_unwrap(self)
            }
        }
    };
}

// Outside the `coverage(off)` test module, so that this expansion of the
// macro counts towards coverage.
#[cfg(all(test, feature = "unit-tests"))]
mod sample {
    use std::collections::BTreeMap;

    joinable_struct! {
        /// One field per kind of lifting, the option spelled as a path.
        source: Sample,
        /// Result-domain form of [`Sample`].
        result: ResultSample,
        fields: {
            pub scalar: u8,
            pub option: std::option::Option<u8>,
            pub map: BTreeMap<u8, u8>,
        }
    }
}

#[cfg(all(test, feature = "unit-tests"))]
#[cfg_attr(coverage_nightly, coverage(off))]
mod tests {
    use std::collections::BTreeMap;

    use super::sample::Sample;
    use crate::lattice::join::Join;
    use crate::lattice::lift::ResultValue;

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
        assert!(ResultValue::is_ok(&joined));
        assert_eq!(ResultValue::try_unwrap(joined).ok(), Some(sample(1, 2, 3)));
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
        assert_eq!(ResultValue::try_unwrap(joined.clone()), Err(joined));
    }
}
