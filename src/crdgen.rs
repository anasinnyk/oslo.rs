use kube::CustomResourceExt;
fn main() {
    print!("---\n{}", vec![
        serde_yaml::to_string(&controller::Service::crd()).unwrap(),
        serde_yaml::to_string(&controller::SLO::crd()).unwrap(),
        serde_yaml::to_string(&controller::SLI::crd()).unwrap(),
        serde_yaml::to_string(&controller::DataSource::crd()).unwrap(),
        serde_yaml::to_string(&controller::AlertPolicy::crd()).unwrap(),
        serde_yaml::to_string(&controller::AlertCondition::crd()).unwrap(),
        serde_yaml::to_string(&controller::AlertNotificationTarget::crd()).unwrap(),
    ].join("---\n"));
}
