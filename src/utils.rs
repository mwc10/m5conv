#[inline(always)]
pub(crate) fn rmap2<T, U, V, E, F>(r1: Result<T, E>, r2: Result<U, E>, f: F) -> Result<V, E>
where
    F: FnOnce(T, U) -> V,
{
    r1.and_then(|r1| r2.map(|r2| f(r1, r2)))
}
