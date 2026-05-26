use std::mem::{ManuallyDrop, size_of};
use std::slice;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct VecLayout {
    pub(crate) ptr_offset: usize,
    pub(crate) len_offset: usize,
    pub(crate) cap_offset: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct OptionStringLayout {
    pub(crate) same_size_niche: bool,
    pub(crate) some_string: VecLayout,
    pub(crate) none_tag_offset: usize,
    pub(crate) none_tag_value: usize,
}

pub(crate) fn string_layout() -> Option<VecLayout> {
    let mut value = ManuallyDrop::new(String::with_capacity(29));
    value.push_str("binette");
    probe_three_word_layout(
        &*value,
        value.as_ptr() as usize,
        value.len(),
        value.capacity(),
    )
}

pub(crate) fn vec_layout() -> Option<VecLayout> {
    let mut value = ManuallyDrop::new(Vec::with_capacity(31));
    value.extend_from_slice(b"binette");
    probe_three_word_layout(
        &*value,
        value.as_ptr() as usize,
        value.len(),
        value.capacity(),
    )
}

pub(crate) fn option_string_layout() -> Option<OptionStringLayout> {
    let mut value = String::with_capacity(37);
    value.push_str("binette option");
    let ptr = value.as_ptr() as usize;
    let len = value.len();
    let cap = value.capacity();
    let option = ManuallyDrop::new(Some(value));
    let some_string = probe_three_word_layout(&*option, ptr, len, cap)?;
    let none = ManuallyDrop::new(None::<String>);
    let none_words = unsafe { words_of(&*none) };
    let tag_index = some_string.cap_offset / size_of::<usize>();
    let none_tag_value = *none_words.get(tag_index)?;
    if none_tag_value == cap {
        return None;
    }

    Some(OptionStringLayout {
        same_size_niche: size_of::<Option<String>>() == size_of::<String>(),
        some_string,
        none_tag_offset: some_string.cap_offset,
        none_tag_value,
    })
}

#[cfg(test)]
pub(crate) fn vec_u8_layout() -> Option<VecLayout> {
    vec_layout()
}

fn probe_three_word_layout<T>(value: &T, ptr: usize, len: usize, cap: usize) -> Option<VecLayout> {
    if size_of::<T>() != size_of::<usize>() * 3 {
        return None;
    }

    let words = unsafe { words_of(value) };
    Some(VecLayout {
        ptr_offset: find_unique_word(words, ptr)? * size_of::<usize>(),
        len_offset: find_unique_word(words, len)? * size_of::<usize>(),
        cap_offset: find_unique_word(words, cap)? * size_of::<usize>(),
    })
}

unsafe fn words_of<T>(value: &T) -> &[usize] {
    unsafe {
        slice::from_raw_parts(
            std::ptr::from_ref(value).cast::<usize>(),
            size_of::<T>() / size_of::<usize>(),
        )
    }
}

fn find_unique_word(words: &[usize], needle: usize) -> Option<usize> {
    let mut matches = words
        .iter()
        .enumerate()
        .filter_map(|(index, word)| (*word == needle).then_some(index));
    let index = matches.next()?;
    matches.next().is_none().then_some(index)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn probes_vec_u8_layout() {
        let layout = vec_u8_layout().unwrap();
        assert_eq!(layout.ptr_offset % size_of::<usize>(), 0);
        assert_eq!(layout.len_offset % size_of::<usize>(), 0);
        assert_eq!(layout.cap_offset % size_of::<usize>(), 0);
        assert_ne!(layout.ptr_offset, layout.len_offset);
        assert_ne!(layout.ptr_offset, layout.cap_offset);
        assert_ne!(layout.len_offset, layout.cap_offset);
    }

    #[test]
    fn probes_string_layout() {
        assert_eq!(string_layout(), vec_u8_layout());
    }

    #[test]
    fn probes_option_string_layout() {
        let layout = option_string_layout().unwrap();
        assert!(layout.same_size_niche);
        assert_eq!(layout.some_string, string_layout().unwrap());
    }
}
