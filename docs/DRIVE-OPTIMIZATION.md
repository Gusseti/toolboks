# Drive optimization in Disk Manager

Disk Manager now runs as an executable from `toolboks-addons`, not inside the host binary.
The protocol 2 pilot keeps fragmentation analysis and its existing elevation request, but
**Defragment, TRIM and Optimize with Windows are disabled in both UI and backend** pending
#132's separate elevated helper and native confirmation window. Repin the host dependencies
and rebuild after #132 merges to staging for protocol 3. No replacement dialog is implemented
by this pilot. The maintenance behavior described below is the original implementation and
must be revalidated after #132 before it is enabled for executable distribution.

Open Disk Manager > Optimization. This view is available without running a folder scan.
It reads local fixed-drive metadata and retained Microsoft-Windows-Defrag success
records, and shows the last recorded defrag and TRIM separately in local time.
Analysis/shrink records are excluded from optimization history. Missing history
means no matching record is available, not that optimization has never occurred.
Only recognized defrag/TRIM operation names are classified. Windows may rotate
logs, drive letters can change, and other optimization programs may not log here.

Analyze fragmentation uses Win32_Volume.DefragAnalysis and its numeric report,
not parsing translated console text. Total, file and free-space fragmentation are
separate nullable values. Reports are cached locally by volume identity and labeled
with their analysis time. A missing measurement is never displayed as zero.

HDDs offer Defragment (`defrag /D`); SSDs with a reported TRIM capability and no
seek penalty offer TRIM (`defrag /L`). If those read-only device queries do not
establish the media type, the action is Optimize with Windows (`defrag /O`).
Storage controller/virtualization support can affect media detection. The Windows
engine chooses behavior for the fallback; Toolboks never guesses HDD from a name.

Administrator access is required for analysis and optimization. Selecting an action starts
only a small helper through UAC; the app and Disk Manager page stay unelevated. The helper
checks the selected volume and shows its own native window explaining the exact action,
with Confirm and Cancel. Host clicks or keys cannot approve it. Cancel performs no drive work.
The helper rechecks volume identity, permissions, media type, filesystem support and the dirty
bit after confirmation, immediately before starting. Only validated
uppercase drive letters become arguments to the system executable; user paths
and command fragments are never accepted as optimization targets.

Windows performs operations in a hidden background process. The tab remains responsive and
disables duplicate actions while waiting for the helper. The final Windows report returns with
the operation result; live phase percentages are not streamed over the helper connection. Operations can continue if Toolboks is closed;
this version does not expose an in-app stop control. After success, history is
refreshed and a fresh analysis attempted. A failed post-analysis does not turn an
otherwise successful optimization into a false failure or show an old report as new.

Disk Manager retains the `drive_optimize` capability and requests it in the catalog install
review. The old 0.1/0.2 local activation packages no longer activate a compiled-in provider.
Saved settings, selected drive, cached reports and the add-on data folder keep the same ID.
Third-party declarative packages cannot request native capabilities or self-authorize a provider.

Help includes five new guided steps. Tour sample data is clearly labeled and never
starts a drive-information query, fragmentation analysis, defrag, TRIM or elevation.

Validation includes operation-flag selection, rejecting invalid targets, drive/media
and permission rechecks, report range checks, history classification, native package
permission migration, every guided target and real read-only inventory/history/media
queries on this machine. HDD and SSD action labels and dates were checked in the UI.
Real defrag/TRIM execution was intentionally not tested, and no UAC prompt was accepted.

References:
- https://learn.microsoft.com/en-us/windows-server/administration/windows-commands/defrag
- https://learn.microsoft.com/en-us/previous-versions/windows/desktop/vdswmi/defraganalysis-method-in-class-win32-volume
- https://learn.microsoft.com/en-us/windows/win32/api/winioctl/ns-winioctl-device_seek_penalty_descriptor
