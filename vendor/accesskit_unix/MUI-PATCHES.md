# MUI Linux provider lifetime patch

Based on AccessKit `accesskit_unix` 0.22.1, upstream commit
`c88605b96d04431f9c3c792464a0f2f253480e94` (`platforms/unix`).
Original source copyright and MIT/Apache-2.0 notices are retained; the copies
of those licenses come from the same AccessKit distribution.

MUI changes only provider/executor lifetime and its dependency routing:

- Each generation has an application context and owned current-thread Tokio
  runtime. Adapters share an owner; native callbacks carry only that generation's
  message sender. A weak registry never keeps a generation alive.
- Last adapter drop requests cancellation of setup/event processing and joins
  the worker after releasing its public state. Runtime drop drains all tasks and
  blocking jobs. There is no timeout which abandons library code.
- Callbacks must not wait on the thread closing the final adapter. A last drop
  from the worker itself requests shutdown without self-joining; shutdown ends
  when its callback returns. Unloading from an executing callback is unsupported.
- Public adapter signatures, upstream version and Rust 1.85 MSRV are retained.
  The historical `async-io` and `tokio` feature names both select the owned Tokio
  implementation. They do not initialize the global async-io reactor here.

The Linux two-library fixture exercises idle/activated generations, real native
AT-SPI actions, two adapters per image, handler destruction, disconnect, reopen
and library unload. A registry-version negative control detects retained workers.
