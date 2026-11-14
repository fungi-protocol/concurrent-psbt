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
macro_rules! joinable_struct {
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
