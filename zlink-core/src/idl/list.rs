//! List type for holding either borrowed or owned collections.

use alloc::vec::Vec;

/// A list that can be either borrowed or owned.
///
/// This type is useful for const contexts where we need borrowed data,
/// as well as for deserialization where we need owned data.
#[derive(Debug, Clone, Eq)]
pub enum List<'a, T> {
    /// Borrowed slice of references, useful for const contexts.
    Borrowed(&'a [&'a T]),
    /// Owned vector, used for deserialization.
    Owned(Vec<T>),
}

impl<'a, T> List<'a, T> {
    /// Returns an iterator over references to the items.
    pub fn iter(&self) -> impl Iterator<Item = &T> {
        match self {
            List::Borrowed(slice) => ListIter::Borrowed(slice.iter()),
            List::Owned(vec) => ListIter::Owned(vec.iter()),
        }
    }

    /// Returns the number of items in the list.
    pub fn len(&self) -> usize {
        match self {
            List::Borrowed(slice) => slice.len(),
            List::Owned(vec) => vec.len(),
        }
    }

    /// Returns true if the list is empty.
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// The borrowed slice of references if this list is borrowed.
    pub const fn as_borrowed(&self) -> Option<&[&T]> {
        match self {
            List::Borrowed(slice) => Some(slice),
            List::Owned(_) => None,
        }
    }

    /// The owned vector of values if this list is owned.
    pub fn as_owned(&self) -> Option<&Vec<T>> {
        match self {
            List::Borrowed(_) => None,
            List::Owned(vec) => Some(vec),
        }
    }
}

/// Iterator over list items.
enum ListIter<'a, T> {
    Borrowed(core::slice::Iter<'a, &'a T>),
    Owned(core::slice::Iter<'a, T>),
}

impl<'a, T> Iterator for ListIter<'a, T> {
    type Item = &'a T;

    fn next(&mut self) -> Option<Self::Item> {
        match self {
            ListIter::Borrowed(iter) => iter.next().copied(),
            ListIter::Owned(iter) => iter.next(),
        }
    }
}

impl<'a, T> Default for List<'a, T> {
    fn default() -> Self {
        List::Borrowed(&[])
    }
}

impl<'a, T> From<Vec<T>> for List<'a, T> {
    fn from(vec: Vec<T>) -> Self {
        List::Owned(vec)
    }
}

impl<'a, T> From<&'a [&'a T]> for List<'a, T> {
    fn from(slice: &'a [&'a T]) -> Self {
        List::Borrowed(slice)
    }
}

impl<'a, T> PartialEq for List<'a, T>
where
    T: PartialEq,
{
    fn eq(&self, other: &Self) -> bool {
        if self.len() != other.len() {
            return false;
        }

        self.iter().zip(other.iter()).all(|(a, b)| a == b)
    }
}

#[cfg(test)]
mod tests {
    use alloc::{
        string::{String, ToString},
        vec,
    };

    use super::*;

    #[test]
    fn list_borrowed() {
        static ITEM_ONE: &str = "one";
        static ITEM_TWO: &str = "two";
        static ITEM_THREE: &str = "three";
        static ITEMS: [&'static &str; 3] = [&ITEM_ONE, &ITEM_TWO, &ITEM_THREE];
        let list: List<'_, &str> = List::Borrowed(&ITEMS);

        assert_eq!(list.len(), 3);
        assert!(!list.is_empty());

        let expected = ["one", "two", "three"];
        let mut actual = Vec::<&str>::new();
        for item in list.iter() {
            actual.push(*item);
        }
        assert_eq!(actual.as_slice(), &expected);
    }

    #[test]
    fn list_owned() {
        let vec = vec!["one".to_string(), "two".to_string(), "three".to_string()];
        let list: List<'_, String> = List::from(vec);

        assert_eq!(list.len(), 3);
        assert!(!list.is_empty());

        let collected: Vec<_> = list.iter().map(|s| s.as_str()).collect();
        assert_eq!(collected, vec!["one", "two", "three"]);
    }

    #[test]
    fn list_partial_eq_cross_variant() {
        // Test that borrowed and owned variants compare equal when they have the same content
        static ITEM_ONE: &str = "one";
        static ITEM_TWO: &str = "two";
        static ITEM_THREE: &str = "three";
        static REFS: [&'static &str; 3] = [&ITEM_ONE, &ITEM_TWO, &ITEM_THREE];
        let borrowed_list: List<'_, &str> = List::Borrowed(&REFS);

        let owned_list: List<'_, &str> = List::Owned(vec!["one", "two", "three"]);
        assert_eq!(borrowed_list, owned_list);
        assert_eq!(owned_list, borrowed_list);

        // Test that lists with different content are not equal
        static OTHER_REFS: [&'static &str; 2] = [&ITEM_ONE, &ITEM_TWO];
        let different_borrowed: List<'_, &str> = List::Borrowed(&OTHER_REFS);
        assert_ne!(borrowed_list, different_borrowed);

        let different_owned: List<'_, &str> = List::Owned(vec!["one", "two"]);
        assert_ne!(borrowed_list, different_owned);
    }
}
