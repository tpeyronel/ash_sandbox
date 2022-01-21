use std::ffi::CStr;

pub struct Image2D {
        data: Vec<u8>,
        width: u32,
        height: u32,
        channels: u32,
}

impl Image2D {
        #[allow(dead_code)]
        pub fn new(path: &CStr, desired_channels: u32) -> Result<Image2D, std::io::Error> {
                let mut width: i32 = 0;
                let mut height: i32 = 0;
                let mut original_channels: i32 = 0;

                let data = unsafe {
                        stb_image::stb_image::bindgen::stbi_load(
                                path.as_ptr(),
                                &mut width as *mut _,
                                &mut height as *mut _,
                                &mut original_channels as *mut _,
                                desired_channels as i32,
                        )
                };

                if data == std::ptr::null_mut() {
                        return Err(std::io::Error::new(
                                std::io::ErrorKind::Other,
                                format!("Error occurred reading file {}", path.to_str().unwrap()),
                        ));
                }

                let data_len = (width as usize) * (height as usize) * (desired_channels as usize);

                Ok(Self {
                        data: unsafe { Vec::from_raw_parts(data, data_len, data_len) },
                        width: width as u32,
                        height: height as u32,
                        channels: desired_channels,
                })
        }

        #[allow(dead_code)]
        pub fn data(&self) -> &[u8] {
                self.data.as_slice()
        }

        #[allow(dead_code)]
        pub fn data_raw(&self) -> *const u8 {
                self.data.as_ptr()
        }

        #[allow(dead_code)]
        pub fn data_bsize(&self) -> usize {
                self.data.len() * std::mem::size_of::<u8>()
        }

        #[allow(dead_code)]
        pub fn width(&self) -> u32 {
                self.width
        }

        #[allow(dead_code)]
        pub fn height(&self) -> u32 {
                self.height
        }

        #[allow(dead_code)]
        pub fn channels(&self) -> u32 {
                self.channels
        }
}
