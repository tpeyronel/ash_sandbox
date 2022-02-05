use ash::vk;
use thiserror::Error;

#[derive(Error, Debug)]
pub enum VkmaError {
        #[error(transparent)]
        VkError(#[from] vk::Result),
        #[error(transparent)]
        VmaError(#[from] vma::Error),
}

pub type VkmaResult<T> = Result<T, VkmaError>;
