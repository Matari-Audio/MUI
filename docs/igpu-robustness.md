# Integrated GPU robustness (implementation, not hardware qualification)

Baseline: `9d1059cc` (`fix/gpu-crash-fallback`). The field report has no logs;
none of these mechanisms establishes the cause of that particular crash.
Coordinator owns all builds, tests and real-device qualification.

## Before-change contracts

| Path / format | Thread and overlap | Owner: create → adopt/reject → retire/drop | Bounds / overload / recovery | Distinguishing proof |
|---|---|---|---|---|
| Native editor startup, CLAP/VST3/AU/standalone | GPU init worker; multiple editors may initialize concurrently | Session owns instance/device; each candidate owns pending trial and pipelines until accepted; rejected resources drop on initializer | Finite backend/adapter list; unsupported limits/formats reject before device request; next backend then CPU | Headless ranking/limit/candidate tests; real Windows Intel first-frame journal |
| Vello and MUI pipeline creation, every GPU consumer | Init workers; resize/presenter setup may overlap other editors | Shared device owns shader cache; renderer owns pipelines | DX12 compilation serialized across MUI/Vello pipelines; checked shaders and workgroup zeroing | Headless shader-source regression; GNU release FXC and MSVC DXC runs |
| Surface configure/resize/present, native hosts | Window callback; rendering worker where supported; never audio callback | Host owns surface and matching config; replacement validates anew | Zero size skipped; caps select format/alpha/present; lost device rebuild or CPU, lost surface recreate | Surface-selection tests; native_editor smoke and forced loss/reopen |
| Offscreen readback, Windows native CPU presentation of GPU-rendered frames | Editor render worker | Offscreen owns COPY_DST/MAP_READ buffer; map view drops before unmap | Bounded bitmap dimensions and map deadline; errors return to existing CPU recovery | Intel/AMD shared-memory resize/damage/readback runs |

Audio execution paths, parameter IDs and session state are unchanged. Rendering
retains automatic damage and existing display pacing; no frame cap is added.
