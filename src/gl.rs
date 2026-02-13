mod gl_inner {
    #![allow(clippy::all)]

    use std::ffi::CStr;

    /// Converts a pointer to a rust string slice.
    ///
    /// # Panics
    /// If the pointer does not point to a valid string slice or if the content
    /// of the string is not UTF8
    pub fn get_c_string_unchecked(ptr: *const u8) -> &'static str {
        unsafe {
            CStr::from_ptr(ptr.cast())
                .to_str()
                .expect("CStr is not UTF8")
        }
    }

    /// Converts a pointer to a rust string slice.
    ///
    /// Unlike [`get_c_string_unchecked`], this will never panic: it will instead
    /// return an empty string if the pointer is invalid or the format is not
    /// UTF8.
    pub fn get_c_string(ptr: *const u8) -> &'static str {
        unsafe {
            (!ptr.is_null())
                .then(|| CStr::from_ptr(ptr.cast()).to_str().ok())
                .flatten()
                .unwrap_or_default()
        }
    }

    pub fn get_gl_string_unchecked(var: types::GLenum) -> &'static str {
        let ptr = unsafe { GetString(var) };
        get_c_string_unchecked(ptr)
    }

    pub fn get_gl_string(var: types::GLenum) -> &'static str {
        let ptr = unsafe { GetString(var) };
        get_c_string(ptr)
    }

    include!(concat!(env!("OUT_DIR"), "/gl_bindings.rs"));
}

pub static mut GL_SHADER_STORAGE_BUFFER_OFFSET_ALIGNMENT: i32 = 0;

pub fn align_to_gl_ssbo(value: i32) -> i32 {
    let ssbo_align = unsafe { GL_SHADER_STORAGE_BUFFER_OFFSET_ALIGNMENT };
    (value + ssbo_align - 1) & !(ssbo_align - 1)
}

pub use self::gl_inner::*;
