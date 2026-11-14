# Unreleased

## Changed

- **Shared code for add-ons (#99, no change in behaviour):** Audio Gainer, Macro Manager and System Profiles now import the shared crates directly and no host code. `data_root` and `write_atomic` moved to `toolboks-shared` (`storage`); the host re-exports them. The rest of the Debloater engine (apps, restore points, services, startup, scheduled tasks, and the change error with its rollback) moved to `toolboks-debloat-engine`; the Debloater page uses it from there.
