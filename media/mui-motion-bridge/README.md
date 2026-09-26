# MUI Motion bridge

Optional development host for a persistent native MUI editor and its real audio engine. Shared services include ordered input, sample-clocked editor frames, automatic component discovery, native layout overrides, cached surface capture, and changed-texture streaming.

The independent `tone` example and Kurv adapter use the same infrastructure. Normal MUI builds do not depend on this crate. The existing `mui-motion` crate continues to own spring/curve mathematics.

See [the integration and scripting guide](../../tools/film/README.md).
