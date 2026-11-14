//! Generates typed conflict enums for the external PSBT types.
//!
//! Temporary until `joinable_struct!` defines the source structs and generates
//! their conflict API from the same field list.

macro_rules! record_conflicts {
    (
        $Result:ident, $Conflict:ident, $Field:ident {
            $( $Variant:ident => $field:ident $( [ $Key:ty ] )? : $Value:ty ),+ $(,)?
        }
    ) => {
        #[derive(Debug, Clone, Copy, PartialEq, Eq)]
        pub enum $Field<'a> {
            $( $Variant $( (&'a $Key) )?, )+
        }

        #[derive(Debug, Clone, Copy)]
        pub enum $Conflict<'a> {
            $( $Variant { $( key: &'a $Key, )? values: &'a $crate::Conflict<$Value> }, )+
        }

        impl<'a> $Conflict<'a> {
            pub fn path(&self) -> $Field<'a> {
                match self {
                    $( Self::$Variant { $( key: record_conflicts!(@key $Key, key), )? .. } => {
                        record_conflicts!(@path $Field $Variant key $( [ $Key ] )?)
                    }, )+
                }
            }
        }

        impl $Result {
            /// Borrow each conflicted field, retaining its keydata and value type.
            pub fn conflicts(&self) -> impl Iterator<Item = $Conflict<'_>> {
                // Exhaustive destructuring requires a mapping for every field.
                let Self { $( $field, )+ } = self;
                std::iter::empty()
                    $( .chain($crate::lattice::conflicts::Conflicts::conflicts($field)
                        .map(record_conflicts!(@item $Conflict $Variant $( [ $Key ] )?))) )+
            }
        }

        impl $crate::lattice::conflicts::Conflicts for $Result {
            type Item<'a> = $Conflict<'a>;

            fn conflicts(&self) -> impl Iterator<Item = Self::Item<'_>> {
                Self::conflicts(self)
            }
        }
    };
    (@key $Key:ty, $key:ident) => { $key };
    (@path $Field:ident $Variant:ident $key:ident [ $Key:ty ]) => { $Field::$Variant(*$key) };
    (@path $Field:ident $Variant:ident $key:ident) => { $Field::$Variant };
    (@item $Conflict:ident $Variant:ident [ $Key:ty ]) => {
        |(key, values)| $Conflict::$Variant { key, values }
    };
    (@item $Conflict:ident $Variant:ident) => {
        |values| $Conflict::$Variant { values }
    };
}
