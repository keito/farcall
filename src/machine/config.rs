#[derive(Clone, Copy, Default, clap::ValueEnum)]
pub(crate) enum MachineModel {
    #[default]
    #[value(name = "pc")]
    IbmPc,
    #[value(name = "at")]
    IbmAt,
}

#[derive(Clone, Copy, Default)]
pub(crate) struct MachineConfig {
    pub model: MachineModel,
}
