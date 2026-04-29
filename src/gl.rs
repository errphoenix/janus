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

pub fn barrier_all() {
    unsafe {
        MemoryBarrier(ALL_BARRIER_BITS);
    }
}

pub fn barrier_queries() {
    unsafe {
        MemoryBarrier(QUERY_BUFFER_BARRIER_BIT);
    }
}

pub fn barrier_buffer_updates() {
    unsafe {
        MemoryBarrier(BUFFER_UPDATE_BARRIER_BIT);
    }
}

pub fn barrier_commands() {
    unsafe {
        MemoryBarrier(COMMAND_BARRIER_BIT);
    }
}

pub fn barrier_texture_updates() {
    unsafe {
        MemoryBarrier(TEXTURE_UPDATE_BARRIER_BIT);
    }
}

pub fn barrier_shader_image() {
    unsafe {
        MemoryBarrier(SHADER_IMAGE_ACCESS_BARRIER_BIT);
    }
}

pub fn barrier_texture_fetch() {
    unsafe {
        MemoryBarrier(TEXTURE_FETCH_BARRIER_BIT);
    }
}

pub fn barrier_uniforms() {
    unsafe {
        MemoryBarrier(UNIFORM_BARRIER_BIT);
    }
}

pub fn barrier_vertex_attributes() {
    unsafe {
        MemoryBarrier(VERTEX_ATTRIB_ARRAY_BARRIER_BIT);
    }
}

pub fn barrier_elements() {
    unsafe {
        MemoryBarrier(ELEMENT_ARRAY_BARRIER_BIT);
    }
}

pub fn barrier_framebuffers() {
    unsafe {
        MemoryBarrier(FRAMEBUFFER_BARRIER_BIT);
    }
}

pub fn barrier_atomics() {
    unsafe {
        MemoryBarrier(ATOMIC_COUNTER_BARRIER_BIT);
    }
}

pub fn barrier_shader_storage() {
    unsafe {
        MemoryBarrier(SHADER_STORAGE_BARRIER_BIT);
    }
}
