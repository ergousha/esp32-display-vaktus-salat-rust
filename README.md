# Vaktu's-Salât — ESP32 Rust Prayer Times Dashboard

A `no_std`-free (std, ESP-IDF-based) Rust firmware for a 4.0" ESP32 TFT board
that connects to WiFi, syncs the clock over NTP, downloads the month's prayer
times from the [ezanvakti.emushaf.net](https://ezanvakti.emushaf.net/) API for
an on-device, searchable country/city/district selection (Haarlem,
Netherlands by default), and shows a full-screen dashboard: the 5 daily
vakits, the currently upcoming one highlighted, and a live countdown until it.
Tap the touchscreen anywhere to toggle the header's date between Miladi
(Gregorian) and Hijri.

![Dashboard preview](docs/dashboard-preview.png)

Tapping the gear icon opens the settings screen — language, header date mode,
and the WiFi / touch-calibration / About / Location system actions:

![Settings screen preview](docs/settings-preview.png)

The System actions each open their own touch-driven page, returning to
Settings via the top-right back arrow. Location, for instance, walks through a
typed search at each level of the API hierarchy — country, then city, then
(where the city itself isn't specific enough) district — using the same
on-screen keyboard as WiFi setup:

![Location search preview](docs/location-search-preview.png)

## Hardware

Board: **4.0 inch ESP32-32E Display** (also silkscreened as E32R40T /
E32N40T). Product page / vendor wiki with schematics and datasheets:
<https://www.lcdwiki.com/4.0inch_ESP32-32E_Display>

- **MCU**: ESP32 (Xtensa LX6, dual core @ 240MHz), the classic/original
  ESP32, not S2/S3/C-series. Revision v3.1, 4MB flash, **no PSRAM**.
- **Display controller**: ST7796S, driven over SPI (SPI2/HSPI bus),
  320×480 px physically (portrait), RGB565 color, BGR subpixel order.
  This firmware rotates it 90° in software to a 480×320 landscape canvas.
- **Touch controller**: XPT2046 (resistive), shares the same SPI2 bus as the
  display with its own CS line. Reads both pressure (for tap detection — e.g.
  toggling the header's date mode) and raw X/Y coordinates (fed through a
  per-unit affine calibration for touch-driven UI) — see
  [`src/touch.rs`](src/touch.rs) and
  [`src/touch_calibration.rs`](src/touch_calibration.rs).

None of this pinout is silkscreened or in the vendor wiki's text; it was
reverse-engineered from the sibling C project's `sdkconfig`
(`CONFIG_LV_DISP_*` / `CONFIG_LV_TOUCH_*` keys). Keep this table in sync if
you ever target a different board revision.

| Function                | GPIO | Notes |
| ------------------------ | ---- | ----- |
| Display MOSI             | 13   | shared SPI2 bus |
| Display SCLK             | 14   | shared SPI2 bus |
| Display CS               | 15   | |
| Display DC (data/cmd)    | 2    | |
| Display Backlight        | 27   | driven via LEDC PWM (`BACKLIGHT_DUTY_PERCENT` in `main.rs`), not a plain GPIO switch |
| Display Reset            | —    | not wired; tied high, unused in software (`mipidsi`'s `NoResetPin`) |
| Touch (XPT2046) MISO     | 12   | shared SPI2 bus |
| Touch (XPT2046) MOSI/CLK | 13/14| same lines as the display |
| Touch (XPT2046) CS       | 33   | |
| Touch (XPT2046) IRQ      | 36   | not wired up in software — this firmware polls pressure over SPI instead |
| Touch X calibration      | —    | vendor-default raw ADC range ~110–1971, X inverted (fallback only — see below) |
| Touch Y calibration      | —    | vendor-default raw ADC range ~88–1929, Y inverted (fallback only — see below) |

The X/Y ranges above are reverse-engineered vendor defaults, used **only** as
an emergency fallback (`vendor_default` in
[`logic/src/touch_calibration.rs`](logic/src/touch_calibration.rs)) when the
calibration wizard can't complete. Real coordinates come from a per-unit
5-point affine calibration run on first boot and stored in NVS — resistive
panels vary too much unit-to-unit to rely on fixed constants. See
[Touch calibration](#touch-calibration) below.

Tap detection still reads the Z1/Z2 pressure channels (no calibration
needed). The C driver uses pressure-based detection too
(`CONFIG_LV_TOUCH_DETECT_PRESSURE=y`, not the IRQ pin) — this firmware
follows that same proven approach rather than relying on GPIO36 alone,
since GPIO34-39 on the ESP32 have no internal pull resistors.

### Display quirks worth knowing

- **SPI clock: 80MHz, write-only.** Matches the C driver's own config
  (`CONFIG_LV_TFT_SPI_CLK_DIVIDER_1` = undivided 80MHz APB clock). The panel
  handles this fine; don't be surprised it's much faster than the usual
  "safe" 20-40MHz seen in generic SPI TFT tutorials.
- **Mirrored column order.** This specific panel's MADCTL column-address bit
  is the opposite of what the [`mipidsi`](https://docs.rs/mipidsi) crate
  assumes by default, for *any* rotation. It was root-caused by comparing
  against the C driver's known-good MADCTL bytes (`0x48` portrait /
  `0x28` landscape) — both encode the same fixed-up parity. The fix is
  setting `mirrored: true` on `mipidsi::options::Orientation` regardless of
  which `Rotation` you pick. If you ever change the rotation, keep
  `mirrored: true`.
- **BGR, not RGB** subpixel order (`ColorOrder::Bgr`).
- **Touch pressure threshold is untested on real hardware.** `PRESSURE_THRESHOLD`
  in [`src/touch.rs`](src/touch.rs) was picked from the same heuristic the
  popular PJRC/Adafruit `XPT2046_Touchscreen` Arduino driver uses
  (`z1 + (4095 - z2)`), not calibrated against this specific panel. If taps
  are missed or trigger spuriously, that constant is the first thing to tune.

## Touch calibration

Resistive panels vary too much unit-to-unit (and by how the glass is mounted)
to trust fixed ADC constants, so the raw XPT2046 readings are mapped to logical
(post-rotation, 480×320) screen coordinates through a per-unit **affine
transform** derived by an on-screen wizard:

```text
x_screen = a·x_raw + b·y_raw + c
y_screen = d·x_raw + e·y_raw + f
```

- **First-boot wizard**: runs right after display+touch init, *before* WiFi. It
  draws 5 crosshair targets — 4 corners inset ~12% from the edges (panels are
  least linear right at the border) plus the center — and captures a stable
  averaged tap at each (touch-down → sample → release, so a single long press
  can't skip a point).
- **Solve + verify**: the 4 corners over-determine the 6 coefficients (8
  equations, 6 unknowns), solved by least squares — more noise-robust than an
  exact 3-point solve. The center tap is *verification only*: if the solved
  transform predicts it more than ~15px off, the wizard restarts.
- **Persistence**: on success the 6 coefficients are stored as a JSON blob in
  the `touch`/`calib` NVS namespace (same pattern as the prayer-time cache) and
  reused on every later boot, so the wizard only runs once.
- **Re-calibration**: either **hold the screen for 5 seconds during the boot
  splash**, or tap **Settings → Calibrate touch** at any time. Both clear the
  saved calibration and re-run the wizard.
- **Fallbacks** (never hang): if a target gets no tap within 30s, or
  verification fails 3 full passes in a row, the firmware falls back to the
  documented vendor-default ADC constants (see the pinout table) with a warning
  and keeps running. The fallback is *not* persisted, so the next boot retries
  the wizard.

The affine solve is pure math with no hardware dependency, so it's unit tested
on a normal host toolchain in the `logic` crate (`solve_affine` in
[`logic/src/touch_calibration.rs`](logic/src/touch_calibration.rs)); the
device-facing wizard/NVS/gesture code is in
[`src/touch_calibration.rs`](src/touch_calibration.rs).

## External services

### Prayer times: ezanvakti.emushaf.net

Turkish Diyanet prayer times API, HTTPS, no API key needed. Endpoints:

| Endpoint | Purpose |
| --- | --- |
| `GET /ulkeler` | list of countries |
| `GET /sehirler/{ulkeId}` | cities/regions in a country |
| `GET /ilceler/{sehirId}` | districts in a city/region |
| `GET /vakitler/{ilceId}` | ~32 days of prayer times for a district |

The district id (`ilceId`) is resolved on-device through the Settings →
Location page (see [`src/location_setup.rs`](src/location_setup.rs)): a typed
search at each level narrows the country list, then the city list, then the
district list, rather than rendering the full (often thousand-plus-row) API
response on a resistive-touch panel. Some countries — the Netherlands among
them — expose the whole country as a single pseudo-city (`UlkeID=4` →
`SehirID=721`, named "HOLLANDA") with the real towns one level deeper as
districts, so **Haarlem** is picked at the *district* step
(`IlceID=13877`), not the city step. The confirmed selection (ids + display
names) is persisted to NVS via [`src/location.rs`](src/location.rs) and
defaults to that same Haarlem district on a fresh device.

The firmware caches the fetched month to NVS flash (see "NVS caching of
prayer data" under [Software architecture](#software-architecture) below), so
`/vakitler/{ilceId}` is only actually fetched over HTTPS right after a location
change, on the very first boot, or whenever today's date falls outside the
cached ~32-day range (checked every second, throttled to one attempt per 5
minutes) — in practice about once a month. Of each day's JSON object, only
these fields are used:

- `MiladiTarihKisa` — Gregorian date, `"DD.MM.YYYY"`, used to find "today"'s
  row and shown in the header by default
- `HicriTarihKisa` — Hijri date, `"D.M.YYYY"`, shown in the header instead
  when you tap the touchscreen
- `Imsak`, `Ogle`, `Ikindi`, `Aksam`, `Yatsi` — the 5 vakits, `"HH:MM"`
  (`Gunes`/sunrise is returned too but intentionally not shown — it isn't
  one of the 5 daily prayers)

### Time sync: NTP

`esp_idf_svc::sntp::EspSntp` with ESP-IDF's defaults: `0.pool.ntp.org` through
`3.pool.ntp.org`. The firmware waits up to 20s for a sync at boot before
continuing (it proceeds either way — see [Software architecture](#software-architecture) for
why a failed/slow sync isn't fatal).

**Local time is NOT computed via libc `tzset`/`localtime_r`** — it wasn't
certain those are exposed through every version of the ESP-IDF Rust bindings,
so [`src/time_utils.rs`](src/time_utils.rs) implements Europe/Amsterdam's
CET/CEST offset from scratch (pure calendar math, no dependencies), following
the actual EU DST rule: CEST (UTC+2) from the last Sunday of March 01:00 UTC
to the last Sunday of October 01:00 UTC, CET (UTC+1) otherwise. If you deploy
this outside the EU, that's the function to replace.

## Software architecture

```
src/
├── main.rs        WiFi/SNTP/display/touch/backlight bring-up, main loop, all drawing code
├── settings_screen.rs  Settings screen: language/date options + the 4 System actions
├── keyboard.rs     Shared on-screen keyboard + text-entry screen (WiFi + Location)
├── about.rs        About page: hardware model, firmware version, station MAC, cloud status
├── cloud/          AWS IoT link, on its own thread (see "Cloud link and OTA updates")
│   ├── mod.rs        boot flow: secure element -> provisioning -> MQTT -> Jobs + shadow
│   ├── atecc.rs      ATECC608B setup (lock, slot-0 key), public key, CSR signing
│   ├── provisioning.rs  fleet provisioning by claim with a chip-signed CSR
│   ├── mqtt.rs       esp-mqtt over the raw ESP-IDF API (secure-element TLS)
│   ├── identity.rs   thing name + certificate in NVS
│   ├── jobs.rs, ota.rs, job_store.rs  OTA updates through AWS IoT Jobs
├── location.rs     NVS load/save for the selected location + the Ezan Vakti list fetches
├── location_setup.rs  Touch-driven location picker: country/city/district typed search
├── prayer.rs       HTTPS fetch + JSON model for the ezanvakti API (parameterized by district id)
├── cache.rs        NVS load/save for the fetched prayer-time month
├── touch.rs        XPT2046 touch driver (pressure + raw X/Y coordinates)
├── touch_calibration.rs  NVS persistence + 5-point calibration wizard + gesture
├── wifi_credentials.rs   NVS load/save for the on-device WiFi credentials
├── wifi_setup.rs   Touch-driven WiFi provisioning: scan picker + the shared on-screen keyboard
├── time_utils.rs   Calendar math + Europe/Amsterdam DST offset (no libc)
└── segdisplay.rs    Seven-segment-style big digit renderer (embedded-graphics
                      primitives), used for the countdown clock
```

The pure affine solve/apply math (`Calibration`, least-squares `solve_affine`,
`vendor_default`) lives in the host-testable `logic` crate
([`logic/src/touch_calibration.rs`](logic/src/touch_calibration.rs)); the
`src/` module above is the hardware-facing half (NVS + on-screen wizard).

- **Rendering**: the panel is only repainted where something actually
  changed, not full-screen every tick — an earlier full `clear()` +
  redraw every second took 100-200ms and was visibly flickering.
  - `draw_static_frame` (whole-panel clear): only on the very first frame and
    on day rollover — draws the header separator and all 5 vakit cards.
  - `update_card_highlight`: redraws just the 1-2 cards whose highlight
    state changed (fires a handful of times a day, whenever the upcoming
    vakit changes).
  - `draw_dynamic`: the header line (date/weekday/clock), the "next vakit"
    label, the big countdown and the progress bar. Runs once a **minute**
    (the dashboard only shows minute resolution) and clears only its own
    small bounding box before redrawing, instead of the whole screen.
- **Fonts & localization**: the UI renders in one of three languages — Türkçe,
  English or Nederlands — chosen from the settings screen and persisted to NVS
  (namespace `settings`, keys `lang`/`datemode` as single bytes; see
  [`src/settings.rs`](src/settings.rs)). The `Language` enum and every
  translation table live in the host-testable
  [`namaz-vakti-logic`](logic/src/language.rs) crate.
  - All three are Latin scripts, drawn with embedded-graphics'
    `mono_font::iso_8859_9` (Latin-5) fonts, not the default `ascii` set —
    `ascii` only covers 0x20-0x7E and can't render Turkish letters (İ, ı, Ş, ş,
    Ğ, ğ, Ö, ö, Ü, ü, Ç, ç) at all. See [`src/text.rs`](src/text.rs).
  - An Arabic UI option existed up to v0.7.0. It was removed to free flash for
    the two OTA slots a 4 MB part has room for. A device that still has it
    saved (byte `2`) falls back to Türkçe.
- **Resilience**: if WiFi fails to connect at boot, the device retries the
  saved credentials a bounded number of times (showing a "reconnecting"
  status when there's cached data to fall back on) and then drops into the
  on-device WiFi setup flow — no more reboot-loop on stale credentials. If
  the prayer-time fetch fails, it retries
  a few times with a status screen, then keeps retrying in the background
  (throttled to once per 5 minutes) once the main dashboard is showing.
  A failed/slow NTP sync isn't treated as fatal — in practice the ESP-IDF
  SNTP callback finishes shortly after and updates the clock before it's
  actually needed (the HTTPS TLS handshake's certificate-date check already
  requires a correct clock, so by the time prayer data is fetched the time
  is verified good).
- **Touch input**: the display and touch controller share one SPI2 bus
  (`SpiDriver` wrapped in an `Rc`, since `esp-idf-hal`'s `SpiDeviceDriver`
  only needs to *borrow* the bus — each device gets its own hardware CS
  pin and clock speed: 80MHz write-only for the display, 2MHz full-duplex
  for the touch ADC). On the dashboard, touch is used solely to hit-test a
  gear icon in the top-right of the header (a ~32×32 tap square): tapping it
  opens the settings screen, where the language and header date mode
  (Miladi/Hijri) are selected and immediately persisted. Tapping anywhere
  else on the dashboard does nothing. A raw touch is mapped to a screen pixel
  with the calibrated affine transform (`Calibration::to_screen`) before
  hit-testing.
- **Touch calibration**: see [Touch calibration](#touch-calibration) for the
  first-boot wizard, the raw→screen affine mapping, and re-calibration.
- **NVS caching of prayer data**: the fetched month is JSON-serialized into
  the `namaz`/`days` NVS blob (see [`src/cache.rs`](src/cache.rs)) every time
  a fetch succeeds. At boot, the cache is tried *before* any network
  activity; if it holds data, the dashboard can render as soon as WiFi+NTP
  are ready without waiting on ezanvakti.emushaf.net at all. The blocking
  "indiriliyor..." fetch screen from a completely empty flash (very first
  boot, or a corrupt/erased NVS) is the only time a boot actually waits on
  the HTTPS call.

## Cloud link and OTA updates

The display is the `vaktus` product of the AWS IoT platform in
[`iot-platform-infra`](https://github.com/ergousha/iot-platform-infra), next to
the OPC UA gateway. The cloud link runs on its own thread; the dashboard never
waits for it, and prayer times keep working with no cloud at all.

```mermaid
sequenceDiagram
    participant D as Display (ESP32 + ATECC608B)
    participant IOT as AWS IoT Core
    participant HOOK as Pre-provisioning hook
    participant DB as Device registry
    D->>D: first boot: lock the chip, generate the slot-0 key (one-way)
    D->>D: log REGISTER mac=… public_key=…
    Note over D,DB: operator: seed-device --product vaktus --mac … --public-key …
    D->>IOT: connect with the shared claim certificate
    D->>IOT: CreateCertificateFromCsr (CSR signed inside the chip)
    IOT-->>D: certificate for the chip's key
    D->>IOT: RegisterThing (SerialNumber, MacAddress)
    IOT->>HOOK: pre-provisioning check
    HOOK->>DB: record for this MAC
    HOOK-->>IOT: allow only if product = vaktus and the certificate carries the registered key
    IOT-->>D: thingName
    D->>IOT: connect as the thing; TLS signed by the chip
    D->>IOT: AWS IoT Jobs (OTA), shadow "vaktus" report
```

### Secure element: ATECC608B

An ATECC608B add-on sits on the board's 4-pin I2C header (**SDA GPIO32, SCL
GPIO25**, address 0xC0; the module brings its own pull-ups). The classic ESP32
has no key-protection hardware of its own, so the chip holds the device's
private key: generated inside it, never readable, and the only key the device
ever authenticates with.

- **First boot** (any blank chip): the firmware locks the configuration zone as
  it left the factory (slot 0 is already a P-256 private key for external
  signing, writable only by GenKey), locks the data zone, generates the key in
  slot 0 and locks the slot. **This is one-way**: the chip keeps that key for
  good. Each step checks the lock state first, so a power cut resumes.
- The firmware talks to the chip through Microchip CryptoAuthLib
  (`espressif/esp-cryptoauthlib`, TrustCustom), on a private device context;
  esp-tls uses it for the TLS handshake (`CONFIG_ESP_TLS_USE_SECURE_ELEMENT`).
- `examples/atecc608_probe.rs` is a read-only bench check of a chip (revision,
  serial, lock state, slot configuration, self-test):
  `cargo build --release --example atecc608_probe`.

### Registering a device

The device logs what to register on every boot:

```text
REGISTER product=vaktus mac=38:18:2B:84:56:F4 atecc_serial=… public_key=<128 hex digits>
```

Register it with the gateway repository's tool (AWS credentials needed):

```sh
cargo run -p seed-device --target host-tuple -- \
    --product vaktus --mac 38:18:2B:84:56:F4 --public-key <128 hex digits>
```

Until then provisioning is rejected, and the device retries (1 minute, backing
off to 15); **Settings → About → Cloud** shows `awaiting registration (MAC)`.
There is no shared secret: only the chip holding the registered key can enrol.

### OTA updates

Releases go to `s3://<firmware bucket>/vaktus/firmware_vX.Y.Z.bin`; the
platform turns each upload into an AWS IoT Job for the `esp32-ztp-vaktus-fleet`
thing group only. The device runs the Jobs state machine shared with the
gateway (`iot-jobs`): it downloads the image over HTTPS into the inactive slot,
restarts into it, and the **new** image reports SUCCEEDED once it has reached
AWS IoT and marked itself valid. An image that does not get there within 10
minutes restarts, and the bootloader rolls it back; the previous image then
reports FAILED, `rolled back`. A job whose document names another product is
refused without a download.

### Flash layout

[`partitions.csv`](partitions.csv), 4 MB:

| Partition | Offset | Size | |
| --- | --- | --- | --- |
| `nvs` | `0x9000` | 24 KiB | WiFi, calibration, location, prayer cache, cloud identity, pending OTA job |
| `otadata` | `0xf000` | 8 KiB | which slot boots |
| `phy_init` | `0x11000` | 4 KiB | RF calibration |
| `ota_0` / `ota_1` | `0x20000` / `0x210000` | 1984 KiB each | application slots (image ≈ 1.6 MB) |

OTA never rewrites the partition table or the bootloader, so both are frozen
for units in the field.

### Moving a unit to the OTA layout

Units flashed before v0.8 have a single `factory` partition. Move them over
once, over USB; `nvs` keeps its place, so their settings survive:

```sh
cargo build --release
espflash flash --bootloader target/bootloader.bin --partition-table partitions.csv \
    --target-app-partition ota_0 --erase-parts otadata \
    target/xtensa-esp32-espidf/release/namaz-vakti
```

`--bootloader` matters: espflash's bundled bootloader has no OTA rollback.
[`espflash.toml`](espflash.toml) and the `cargo run` runner pass the same
options.

### Build configuration

`build.rs` reads the platform's settings from AWS SSM with the `aws` CLI
(credentials from `aws-env.sh`, see `aws-env.sh.example`, or the environment):
the shared claim certificate and key into `certs/` and the IoT endpoint and the
`vaktus` provisioning template into `cfg.toml`, all gitignored. Without SSM
access the build still succeeds and the cloud link stays off.

The `iot-jobs` crate comes from the gateway repository through git. If your git
configuration rewrites GitHub HTTPS URLs to SSH (`url.….insteadOf`), let Cargo
use the git CLI: `export CARGO_NET_GIT_FETCH_WITH_CLI=true`.

## Rust / ESP-IDF toolchain setup

This targets the **classic ESP32**, which is Xtensa (not RISC-V) and needs
Espressif's Rust fork instead of upstream `rustc`, plus a full ESP-IDF
checkout (this uses `esp-idf-hal`/`esp-idf-svc`, i.e. `std`, not the
bare-metal `esp-hal`/`no_std` stack).

One-time setup:

```sh
# Xtensa Rust toolchain + LLVM + GCC
cargo install espup --locked
espup install --targets esp32
. ~/export-esp.sh   # run in every new shell before building; sets PATH/LIBCLANG_PATH

# ESP-IDF's build system needs these (native/system packages, e.g. via
# apt or Homebrew — anything that puts them on PATH works)
#   cmake, ninja

# Linker shim required by esp-idf-hal/esp-idf-svc std builds
cargo install ldproxy --locked

# Flashing tool
cargo install espflash --locked
```

`ESP_IDF_TOOLS_INSTALL_DIR = "workspace"` in [`.cargo/config.toml`](.cargo/config.toml)
means the first `cargo build` clones and builds ESP-IDF itself (~1-2GB,
several minutes) into `.embuild/` inside this project directory — no global
install needed, but expect a slow first build.

### macOS-specific gotchas

- **`cmake`/`ninja` via Homebrew**: `brew install cmake ninja`.
- **If `cargo`/`rustc` aren't on `PATH` even after `. ~/export-esp.sh`**:
  Homebrew's `rustup` formula (`brew install rustup`) doesn't create the
  usual `~/.cargo/bin/cargo`/`rustc` proxy shims that `rustup-init` normally
  does, so plain `rustup`-managed toolchains can be invisible to your shell.
  Confirm with `rustup which cargo` (should resolve to the `esp` toolchain
  because of [`rust-toolchain.toml`](rust-toolchain.toml)); if that path
  works but bare `cargo` doesn't, add its directory to `PATH` for the
  session: `export PATH="$HOME/.rustup/toolchains/esp/bin:$PATH"`.
- **No CH340/CP210x driver install needed** on modern macOS (Ventura+) —
  Apple ships native DriverKit support for those chips
  (`AppleUSBCHCOM.dext`/`AppleUSBSLCOM.dext`). If the board doesn't show up
  as `/dev/cu.usbserial-*` after plugging in (see
  [Build & flash](#build--flash) below), suspect the USB cable (many bundled
  cables are charge-only) or port/hub before chasing a driver.

## Configuration (WiFi credentials)

WiFi credentials are set **on the device**: on first boot (or whenever the
saved credentials stop working), the panel drops into a touch-driven setup
flow — scan for a network, tap to pick it, and type the passphrase on an
on-screen keyboard. Credentials are stored in NVS flash (namespace `wifi`,
key `creds`) and reused on every later boot. You can re-run setup any time from
**Settings → WiFi** (e.g. after changing routers or passwords). No reflash is
needed to change networks.

### Optional: seeding credentials at build time (dev/CI)

For a headless bench/CI build — where there's no one to tap through setup — you
can still bake in credentials via [`toml-cfg`](https://github.com/jamesmunns/toml-cfg).
If present, `cfg.toml` only *seeds* NVS on the very first boot (when no
credentials are stored yet); the on-device value always wins afterwards.
`cfg.toml` is gitignored, so the password never ends up in the repo:

```sh
cp cfg.toml.example cfg.toml
# edit cfg.toml with your real SSID/password
```

```toml
# cfg.toml
[namaz-vakti]
wifi_ssid = "YOUR_WIFI_SSID"
wifi_psk = "YOUR_WIFI_PASSWORD"
```

## Build & flash

```sh
. ~/export-esp.sh
cargo build --release
cargo run --release      # flashes (bootloader, partition table, ota_0) and opens the monitor
# or by hand:
espflash flash --port /dev/ttyUSB0 --bootloader target/bootloader.bin \
    --partition-table partitions.csv --target-app-partition ota_0 \
    target/xtensa-esp32-espidf/release/namaz-vakti
espflash monitor --port /dev/ttyUSB0   # optional: view logs over serial
```

A unit still on the old single-app layout needs the one-time move in
[Moving a unit to the OTA layout](#moving-a-unit-to-the-ota-layout).

`--port` can be omitted — `espflash flash` will prompt you to pick from the
detected ports if there's ambiguity. To find it yourself:

- **macOS**: `ls /dev/cu.*` before and after plugging in; the new entry is
  usually `/dev/cu.usbserial-<n>`. If nothing new shows up, it's almost
  always the USB cable (charge-only) or port, not a missing driver — see
  the macOS gotchas above.
- **Linux**: `/dev/ttyUSB0` (or `ttyACM0`) as in the example above; `dmesg
  | tail` after plugging in confirms it.

`espflash monitor` opens an interactive session (`Ctrl+R` reset, `Ctrl+C`
exit) and needs a real terminal with a TTY attached — it'll fail with
"Failed to initialize input reader" if run from a script or a
non-interactive shell.

### Flashing from WSL2

If you're building on Windows under WSL2, the board enumerates as a Windows
COM port and isn't visible to Linux until you attach it with
[usbipd-win](https://github.com/dorssel/usbipd-win):

```powershell
# Windows PowerShell, as Administrator, one-time:
winget install usbipd

usbipd list                              # find the board's BUSID (look for "CH340"/"CP210x"/"USB Serial")
usbipd bind --busid <BUSID>               # one-time per device
usbipd attach --wsl --busid <BUSID>       # run again any time the board is unplugged/replugged
```

It should then show up in WSL as `/dev/ttyUSB0`.

## Possible future work

- A settings screen to pick a different city without recompiling (the
  touch-driven WiFi-setup keyboard and the recalibration menu entry now exist —
  see [Configuration](#configuration-wifi-credentials) and
  [Touch calibration](#touch-calibration)).
- Multiple saved WiFi networks / automatic network switching (today it's a
  single stored credential — "forget and re-enter" via **Settings → WiFi**).
- Actual backlight dimming control (a schedule, an ambient light sensor, or
  just a manually-set day/night level) — the PWM plumbing is already there,
  it's just fixed at `BACKLIGHT_DUTY_PERCENT`.
- Qibla direction (`KibleSaati` is already in the API response, unused).
- Cache eviction/rotation — the NVS blob is simply overwritten on every
  successful fetch, which is fine at this data size (a few KB) but worth
  knowing if you extend what's cached.
