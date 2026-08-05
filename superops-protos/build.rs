fn main() {
    tonic_build::configure()
        .compile_protos(
            &[
                "../protos/common/v1/common.proto",
                "../protos/k8s/v1/k8s.proto",
            ],
            &["../protos"],
        )
        .unwrap();
}
