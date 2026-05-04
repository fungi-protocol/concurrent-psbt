use crate::join::Join;
use crate::partial_join::{JoinResult, PartialJoin};

impl<A, B> Join for (A, B)
where
    A: Join + Clone,
    B: Join + Clone,
{
    fn join(self, other: Self) -> Self {
        (self.0.join(other.0), self.1.join(other.1))
    }
}

impl<A, B> PartialJoin for (A, B)
where
    A: PartialJoin + Clone,
    B: PartialJoin<Error = A::Error> + Clone,
    A::Error: Clone,
{
    type Error = (JoinResult<A>, JoinResult<B>);

    fn try_join(self, other: Self) -> JoinResult<Self> {
        match (self.0.try_join(other.0), self.1.try_join(other.1)) {
            (Ok(a), Ok(b)) => Ok((a, b)),
            ret => Err(ret),
        }
    }
}
