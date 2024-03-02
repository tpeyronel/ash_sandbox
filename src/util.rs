use std::ops::Deref;

#[allow(unused)]
pub fn log_error<E: std::error::Error>(e: E) {
        log::error!("{}", e);
}

#[allow(unused)]
pub fn log_if_error<T, E: std::error::Error>(r: Result<T, E>) {
        if let Err(e) = r {
                log::error!("{}", e);
        }
}

pub trait RefIntoSlice {
        fn ref_into_slice(&self) -> &[Self]
        where
                Self: Sized;
}

impl<T: 'static> RefIntoSlice for T {
        fn ref_into_slice(&self) -> &[Self] {
                std::slice::from_ref(self)
        }
}

pub trait DerefIntoSlice<D> {
        fn deref_into_slice(&self) -> &[D]
        where
                D: Sized;
}

impl<T: Deref<Target = D>, D: 'static> DerefIntoSlice<D> for T {
        fn deref_into_slice(&self) -> &[D] {
                std::slice::from_ref(&**self)
        }
}

pub fn default<T: Default>() -> T {
        Default::default()
}

pub trait RefIntoBytesSlice {
        unsafe fn as_bytes(&self) -> &[u8];
}

impl<T: 'static> RefIntoBytesSlice for T {
        unsafe fn as_bytes(&self) -> &[u8] {
                let data = self as *const _ as *const u8;
                let len = std::mem::size_of::<T>();
                unsafe { std::slice::from_raw_parts(data, len) }
        }
}

pub fn compute_image_stride(width: u32, height: u32, block_size: u32, block_extent: (u32, u32)) -> u32 {
        let (block_width, block_height) = block_extent;

        let block_rows = (width + (block_width - 1)) / block_width;
        let block_cols = (height + (block_height - 1)) / block_height;

        block_rows * block_cols * block_size
}

pub fn compute_image_stride_with_mipmaps(
        mut width: u32,
        mut height: u32,
        block_size: u32,
        block_extent: (u32, u32),
        mipmaps: u32,
) -> u32 {
        let mut stride = 0;
        for _ in 0..mipmaps {
                stride += compute_image_stride(width, height, block_size, block_extent);
                width = 1.max(width / 2);
                height = 1.max(height / 2);
        }
        stride
}
