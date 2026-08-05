pub mod common {
    pub mod v1 {
        tonic::include_proto!("common.v1");
    }
}

pub mod k8s {
    pub mod v1 {
        tonic::include_proto!("k8s.v1");
    }
}
