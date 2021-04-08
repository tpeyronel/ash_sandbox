use ash::vk;
use bitflags::_core::fmt::Formatter;
use core::fmt;
use std::error::Error;
use std::fmt::Display;

#[derive(Debug)]
pub enum VkmaError {
        VkError(vk::Result),
        VmaError(vma::Error),
}

impl Display for VkmaError {
        fn fmt(&self, f: &mut Formatter) -> fmt::Result {
                match self {
                        VkmaError::VkError(err) => err.fmt(f),
                        VkmaError::VmaError(err) => err.fmt(f),
                }
        }
}

impl Error for VkmaError {}

impl From<vk::Result> for VkmaError {
        fn from(vk_res: vk::Result) -> Self {
                Self::VkError(vk_res)
        }
}

impl From<vma::Error> for VkmaError {
        fn from(vma_err: vma::Error) -> Self {
                Self::VmaError(vma_err)
        }
}

pub type VkmaResult<T> = Result<T, VkmaError>;
