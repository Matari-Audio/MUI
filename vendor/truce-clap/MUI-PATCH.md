# Local Truce CLAP 6.3.0 correction

Source: the published `truce-clap` 6.3.0 crate, license metadata retained in
Cargo.toml. MUI changes only successful session and preset loading: call
`clap_host_params.rescan(CLAP_PARAM_RESCAN_VALUES)` after applying restored values.
Both callbacks run on the host main thread. Failed loads do not notify.

This fixes the probe's three recorded clap-validator state-reproducibility
failures. Remove the path patch when a validated upstream release includes the
same behavior. Truce remains the plugin framework; this does not replace its
state envelope, parameter store or audio-thread handoff.
