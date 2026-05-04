use std::ops::Deref;

use crate::lattice::partial::{JoinResult, PartialJoin, Absorb};

/// Avoid treating `Vec<u8>` as `Vec<u8 as PartialJoin>`` by requiring this
/// explicit wrapper instead of a blanket `impl PartialJoin for Vec<T:
/// PartialJoin>`
///
/// The `Vec<u8>` `impl` is as an `IdempotentValue` and does not
/// `JoinResult<Vec<u8>>` because a `Vec<Result<u8, ConflictingValues<u8>>` is
/// less useful than `Result<Vec<u8>, ConflictingValues<Vec<u8>>`.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub(crate) struct VecWrapper<T>(pub(crate) Vec<T>);

impl<T> Deref for VecWrapper<T> {
    type Target = Vec<T>;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum VecJoinError<T: PartialJoin + PartialEq + Eq>
where
    T::Error: std::fmt::Debug + Clone + PartialEq + Eq,
{
    LengthMismatch(Vec<T>, Vec<T>),
    Nested(Vec<JoinResult<T>>),
}

impl<T> Absorb<T> for VecJoinError<T> {
    fn absorb(self, value: Vec<T>) -> Self {
        if let Nested(vec) = self {
            if vec.push(value);
            return Nested()
        }
    }
}
    

impl<V> PartialJoin for VecWrapper<V>
where
    V: PartialJoin + std::fmt::Debug + Clone + PartialEq + Eq,
    V::Error: std::fmt::Debug + Clone + PartialEq + Eq,
{
    type Error = VecJoinError<V>;

    fn try_join(self, other: Self) -> JoinResult<Self> {
        // if self.0 == other.0 {
        //     return Ok(self.clone());
        // };

        if self.0.len() != other.0.len() {
            return Err(VecJoinError::LengthMismatch(
                self.0.clone(),
                other.0.clone(),
            ));
        }

        let mut all_ok = true;
        let new: Vec<_> = std::iter::zip(self.0, other.0)
            .map(|(x, y)| {
                let lub = x.try_join(y);
                all_ok = all_ok && lub.is_ok();
                lub
            })
            .collect();

        if all_ok {
            Ok(VecWrapper(
                new.into_iter()
                    .map(|x| x.expect("verified all nested results are Ok"))
                    .collect(),
            ))
        } else {
            Err(VecJoinError::Nested(new))
        }
    }
}

#[test]
fn test_vec() {
    let a = VecWrapper(vec![1u8]);
    let b = VecWrapper(vec![2u8]);
    let c = VecWrapper(vec![1u8, 2]);
    let d = VecWrapper(vec![1u8, 3]);
    let e = VecWrapper(vec![2u8, 2]);

    assert_eq!(PartialJoin::try_join(a.clone(), a.clone()), Ok(a.clone()));
    assert_eq!(PartialJoin::try_join(c.clone(), c.clone()), Ok(c.clone()));

    assert_eq!(
        PartialJoin::try_join(a.clone(), b.clone()),
        Err(VecJoinError::Nested(vec![Err(
            crate::values::ConflictingValues([1, 2].into())
        )]))
    );

    assert_eq!(
        PartialJoin::try_join(a.clone(), c.clone()),
        Err(VecJoinError::LengthMismatch(a.0.clone(), c.0.clone()))
    );

    assert_eq!(
        PartialJoin::try_join(c.clone(), d.clone()),
        Err(VecJoinError::Nested(vec![
            Ok(1),
            Err(crate::values::ConflictingValues([2, 3].into()))
        ]))
    );

    assert_eq!(
        PartialJoin::try_join(c.clone(), e.clone()),
        Err(VecJoinError::Nested(vec![
            Err(crate::values::ConflictingValues([1, 2].into())),
            Ok(2)
        ]))
    );
}
