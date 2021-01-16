use ash::vk;

pub enum VkmaError {
        VkError(vk::Result),
        VmaError(vma::Error),
}

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
