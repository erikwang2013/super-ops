pub mod common {
    pub mod v1 {
        tonic::include_proto!("common.v1");
    }
}

pub mod k8s {
    // tonic 生成的 service trait 一律返回 `Result<Response<T>, tonic::Status>`，
    // Status 体积 ≥176 字节会触发 clippy 1.98 的 result_large_err；生成代码不可改，
    // 只在此模块内放行，自有代码仍受该 lint 约束。
    #[allow(clippy::result_large_err)]
    pub mod v1 {
        tonic::include_proto!("k8s.v1");
    }
}

pub mod events;
