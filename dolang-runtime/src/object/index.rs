#[inline]
fn relative(len: usize, index: i64) -> Option<usize> {
    if index >= 0 {
        usize::try_from(index).ok()
    } else {
        let len = i64::try_from(len).ok()?;
        usize::try_from(len.checked_add(index)?).ok()
    }
}

#[inline]
pub(crate) fn element(len: usize, index: i64) -> Option<usize> {
    let index = relative(len, index)?;
    (index < len).then_some(index)
}

#[inline]
pub(crate) fn position(len: usize, index: i64) -> Option<usize> {
    let index = relative(len, index)?;
    (index <= len).then_some(index)
}

#[cfg(test)]
mod tests {
    use super::{element, position};

    #[test]
    fn negative_index_wraps_from_the_end() {
        assert_eq!(element(5, -1), Some(4));
        assert_eq!(position(5, -1), Some(4));
    }

    #[test]
    fn negative_index_out_of_bounds_is_none() {
        assert_eq!(element(5, -6), None);
        assert_eq!(position(5, -6), None);
    }

    #[test]
    fn position_allows_index_equal_to_len_but_element_does_not() {
        assert_eq!(element(5, 5), None);
        assert_eq!(position(5, 5), Some(5));
    }

    #[test]
    fn empty_collection_boundary() {
        assert_eq!(element(0, 0), None);
        assert_eq!(position(0, 0), Some(0));
    }
}
