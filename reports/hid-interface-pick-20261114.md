# F2 hid-interface-pick (2026-11-14, approved by Victor, APPROVALS)

Branch t3/hid-interface-pick, base main 7ff47d3. Only src/hid.rs changed.
## What changed
- Feature reports (send and receive) go through `route_feature`. Opened paths that declare the report id go first and alone, as before.
- If no opened path declares it, Bridge also tries sibling interfaces of the same physical device that do declare it:
  - Siblings go first when every opened descriptor was read and none has any Feature item, and the vendor has no quirk.
  - Otherwise the opened paths go first (today's behaviour) and siblings come after. That covers Razer, whose descriptors are never read (ce9046b), and Microsoft and Thermaltake, whose undeclared reads are known to work. A path that works today still gets the report first.
- Siblings: vendor-page (>= 0xff00) collections first, then the rest. They are scanned lazily, once per open device, and kept in `OpenDevice::feature_siblings`. A sibling must:
  - match vendor id, product id and serial from the handle (empty serial = none) and the same physical key;
  - not be open already, pass `list()`'s filters (no FIDO path, no receiver child), hold no FIDO collection and declare a Feature report.
  Siblings carry feature reports only: no readers, no output or input reports. A failed open or parse skips that sibling.
- Physical key:
  - Windows: hidapi `get_container_id`. The null container and the computer container {..-FFFF-FFFFFFFFFFFF} are refused.
  - Linux: `HID_PHYS` from /sys/class/hidraw/<node>/device/uevent with `/inputN` stripped. USB only. Bluetooth (adapter-wide phys) and receiver children get no siblings.
  - macOS / other: no siblings.
- The Linux blocking reader, the reader identity check and the FIDO/blocked lists are untouched. No new device is listed.

Tests: 8 new on synthetic descriptors (route rules, sibling order, USB port, containers, no cross-routing of identical no-serial devices, list filters, FIDO/featureless refused).

## Not done
- Device-list reorder (vendor feature collections first): skipped. Razer summaries carry no `featureReports` (descriptors unread), so it would not help #16/#28. Panel drivers may index `collections` in descriptor order, and that could not be verified as safe.

## Hardware-untested / caveats
- Nothing was run on hardware. Keyboard-protocol #16 (Huntsman V2 TKL, Linux):
  - fixed only if hidraw5 declares an unnumbered (id 0) Feature report, after one EPIPE on hidraw2 per op;
  - if its reports are numbered only, behaviour is unchanged.
- Desktop #28 / mouse-protocol #99 (0x009c, Windows): a sibling MI is tried only after every opened collection fails. Not verified.
- The Linux shim (`physical_device_key`, the uevent read) was not compiled locally: only the msvc target is installed. The pure parser is tested on Windows.
- Sibling list is cached until close/recover. If a send falls through to an opened path, the next receive still tries siblings first (no per-report memory).
