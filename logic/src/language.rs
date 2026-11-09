//! UI language selection and the translation tables for every user-facing
//! string in the firmware.
//!
//! Lives in the pure `namaz-vakti-logic` crate (like `prayer_times` and
//! `time_utils`) so the tables can be unit tested on a host toolchain. The
//! firmware resolves each label through [`Language`] before drawing it. Every
//! language is written in a Latin script, so all of them render with the same
//! ISO 8859-9 mono fonts.

/// The UI languages the dashboard can render in.
///
/// [`Self::to_u8`] is the single-byte value persisted to NVS (see the
/// firmware's `settings` module), so the numbering must stay stable. Byte `2`
/// belonged to Arabic, which was removed to free flash for OTA; a device that
/// still has it stored falls back to the default, like any unknown byte.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Language {
    Turkish,
    English,
    Nederlands,
}

impl Default for Language {
    /// Türkçe is the historical default the firmware shipped with.
    fn default() -> Self {
        Language::Turkish
    }
}

impl Language {
    /// Stable byte used for NVS persistence.
    pub fn to_u8(self) -> u8 {
        match self {
            Language::Turkish => 0,
            Language::English => 1,
            Language::Nederlands => 3,
        }
    }

    /// Inverse of [`Self::to_u8`]; unknown bytes fall back to the default.
    pub fn from_u8(value: u8) -> Self {
        match value {
            1 => Language::English,
            3 => Language::Nederlands,
            _ => Language::Turkish,
        }
    }

    /// The selectable languages, in the order the settings screen lists them:
    /// alphabetical by endonym — English, Nederlands, Türkçe.
    pub const ALL: [Language; 3] = [Language::English, Language::Nederlands, Language::Turkish];
}

/// The 5 daily prayer names, indexed the same as [`crate::prayer_times`]'s
/// `prayers()` (İmsak, Öğle, İkindi, Akşam, Yatsı).
pub fn prayer_names(lang: Language) -> [&'static str; 5] {
    match lang {
        Language::Turkish => ["İMSAK", "ÖĞLE", "İKİNDİ", "AKŞAM", "YATSI"],
        Language::English => ["FAJR", "DHUHR", "ASR", "MAGHRIB", "ISHA"],
        Language::Nederlands => ["FAJR", "DHUHR", "ASR", "MAGHRIB", "ISHA"],
    }
}

/// Weekday names indexed 0 = Sunday .. 6 = Saturday, matching
/// [`crate::time_utils`]'s weekday numbering.
pub fn weekday_names(lang: Language) -> [&'static str; 7] {
    match lang {
        Language::Turkish => [
            "PAZAR",
            "PAZARTESİ",
            "SALI",
            "ÇARŞAMBA",
            "PERŞEMBE",
            "CUMA",
            "CUMARTESİ",
        ],
        Language::English => [
            "SUNDAY",
            "MONDAY",
            "TUESDAY",
            "WEDNESDAY",
            "THURSDAY",
            "FRIDAY",
            "SATURDAY",
        ],
        Language::Nederlands => [
            "ZONDAG",
            "MAANDAG",
            "DINSDAG",
            "WOENSDAG",
            "DONDERDAG",
            "VRIJDAG",
            "ZATERDAG",
        ],
    }
}

/// Every fixed, non-parametric UI string, keyed so each has exactly one
/// translation per language.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Msg {
    // Splash / status
    AppTitle,
    Starting,
    WifiConnecting,
    WifiConnectFailed,
    Restarting,
    TimeSyncing,
    PrayerDownloading,
    PrayerFetchFailed,
    RetryingInBackground,
    PrayerDownloadFailed,
    Retrying,
    PrayerDataMissing,
    // Dashboard
    NextPrayer,
    // Settings screen
    SettingsTitle,
    LanguageHeading,
    DateHeading,
    DateMiladi,
    DateHijri,
    SystemHeading,
    WifiMenu,
    RecalibrateTouch,
    AboutMenu,
    LocationMenu,
    // About page
    AboutTitle,
    AboutHardware,
    AboutFirmware,
    AboutMac,
    // Location selection flow
    LocationTitle,
    LocationSearchCountry,
    LocationSearchCity,
    LocationSearchDistrict,
    LocationLoading,
    LocationNoResults,
    LocationError,
    LocationSaving,
    LocationSaved,
    LocationMore,
    // WiFi setup flow
    WifiSetupTitle,
    WifiScanning,
    WifiSelectNetwork,
    WifiEnterManually,
    WifiRescan,
    WifiNoNetworks,
    WifiEnterSsid,
    WifiEnterPassword,
    WifiReconnecting,
    WifiPasswordTooShort,
    KeySpace,
    KeyDone,
    // Touch calibration wizard
    CalTitle,
    CalTapCrosshair,
    CalComplete,
    CalFailed,
    CalRetry,
    CalSkipped,
    CalUsingDefaults,
    CalRecalibrating,
}

/// Resolves a [`Msg`] to its translation in `lang`.
pub fn text(lang: Language, msg: Msg) -> &'static str {
    use Msg::*;
    match (lang, msg) {
        // --- Turkish ---
        (Language::Turkish, AppTitle) => "Namaz Vakti",
        (Language::Turkish, Starting) => "Başlatılıyor...",
        (Language::Turkish, WifiConnecting) => "WiFi'ye bağlanılıyor...",
        (Language::Turkish, WifiConnectFailed) => "WiFi bağlantısı başarısız",
        (Language::Turkish, Restarting) => "Yeniden başlatılıyor...",
        (Language::Turkish, TimeSyncing) => "Saat senkronize ediliyor...",
        (Language::Turkish, PrayerDownloading) => "Namaz vakitleri indiriliyor...",
        (Language::Turkish, PrayerFetchFailed) => "Namaz vakitleri alınamadı",
        (Language::Turkish, RetryingInBackground) => "Arka planda tekrar denenecek...",
        (Language::Turkish, PrayerDownloadFailed) => "Namaz vakitleri indirilemedi",
        (Language::Turkish, Retrying) => "Tekrar deneniyor...",
        (Language::Turkish, PrayerDataMissing) => "Namaz vakti verisi eksik, yenileniyor...",
        (Language::Turkish, NextPrayer) => "SIRADAKİ VAKİT:",
        (Language::Turkish, SettingsTitle) => "Ayarlar",
        (Language::Turkish, LanguageHeading) => "Dil",
        (Language::Turkish, DateHeading) => "Tarih",
        (Language::Turkish, DateMiladi) => "Miladi",
        (Language::Turkish, DateHijri) => "Hicri",
        (Language::Turkish, SystemHeading) => "Sistem",
        (Language::Turkish, WifiMenu) => "WiFi",
        (Language::Turkish, RecalibrateTouch) => "Kalibrasyon",
        (Language::Turkish, AboutMenu) => "Hakkında",
        (Language::Turkish, LocationMenu) => "Konum",
        (Language::Turkish, AboutTitle) => "Hakkında",
        (Language::Turkish, AboutHardware) => "Donanım",
        (Language::Turkish, AboutFirmware) => "Yazılım",
        (Language::Turkish, AboutMac) => "MAC adresi",
        (Language::Turkish, LocationTitle) => "Konum",
        (Language::Turkish, LocationSearchCountry) => "Ülke ara",
        (Language::Turkish, LocationSearchCity) => "Şehir ara",
        (Language::Turkish, LocationSearchDistrict) => "İlçe ara",
        (Language::Turkish, LocationLoading) => "Yükleniyor...",
        (Language::Turkish, LocationNoResults) => "Sonuç bulunamadı",
        (Language::Turkish, LocationError) => "Konum verisi alınamadı",
        (Language::Turkish, LocationSaving) => "Vakitler güncelleniyor...",
        (Language::Turkish, LocationSaved) => "Konum kaydedildi",
        (Language::Turkish, LocationMore) => "Aramayı daraltın",
        (Language::Turkish, WifiSetupTitle) => "WiFi Kurulumu",
        (Language::Turkish, WifiScanning) => "Ağlar taranıyor...",
        (Language::Turkish, WifiSelectNetwork) => "Ağınızı seçin",
        (Language::Turkish, WifiEnterManually) => "Elle gir",
        (Language::Turkish, WifiRescan) => "Yeniden tara",
        (Language::Turkish, WifiNoNetworks) => "Ağ bulunamadı",
        (Language::Turkish, WifiEnterSsid) => "Ağ adı",
        (Language::Turkish, WifiEnterPassword) => "Parola",
        (Language::Turkish, WifiReconnecting) => "Yeniden bağlanılıyor...",
        (Language::Turkish, WifiPasswordTooShort) => "Parola çok kısa (en az 8)",
        (Language::Turkish, KeySpace) => "boşluk",
        (Language::Turkish, KeyDone) => "Tamam",
        (Language::Turkish, CalTitle) => "Dokunmatik Kalibrasyon",
        (Language::Turkish, CalTapCrosshair) => "Hedefe kalem ile dokunun",
        (Language::Turkish, CalComplete) => "Kalibrasyon tamamlandı",
        (Language::Turkish, CalFailed) => "Kalibrasyon başarısız",
        (Language::Turkish, CalRetry) => "Lütfen tekrar deneyin",
        (Language::Turkish, CalSkipped) => "Kalibrasyon atlandı",
        (Language::Turkish, CalUsingDefaults) => "Varsayılan değerler kullanılıyor",
        (Language::Turkish, CalRecalibrating) => "Dokunmatik yeniden kalibre ediliyor",

        // --- English ---
        (Language::English, AppTitle) => "Prayer Times",
        (Language::English, Starting) => "Starting...",
        (Language::English, WifiConnecting) => "Connecting to WiFi...",
        (Language::English, WifiConnectFailed) => "WiFi connection failed",
        (Language::English, Restarting) => "Restarting...",
        (Language::English, TimeSyncing) => "Synchronizing clock...",
        (Language::English, PrayerDownloading) => "Downloading prayer times...",
        (Language::English, PrayerFetchFailed) => "Could not fetch prayer times",
        (Language::English, RetryingInBackground) => "Retrying in the background...",
        (Language::English, PrayerDownloadFailed) => "Prayer times download failed",
        (Language::English, Retrying) => "Retrying...",
        (Language::English, PrayerDataMissing) => "Prayer data missing, refreshing...",
        (Language::English, NextPrayer) => "NEXT PRAYER:",
        (Language::English, SettingsTitle) => "Settings",
        (Language::English, LanguageHeading) => "Language",
        (Language::English, DateHeading) => "Date",
        (Language::English, DateMiladi) => "Gregorian",
        (Language::English, DateHijri) => "Hijri",
        (Language::English, SystemHeading) => "System",
        (Language::English, WifiMenu) => "WiFi",
        (Language::English, RecalibrateTouch) => "Calibrate",
        (Language::English, AboutMenu) => "About",
        (Language::English, LocationMenu) => "Location",
        (Language::English, AboutTitle) => "About",
        (Language::English, AboutHardware) => "Hardware",
        (Language::English, AboutFirmware) => "Firmware",
        (Language::English, AboutMac) => "MAC address",
        (Language::English, LocationTitle) => "Location",
        (Language::English, LocationSearchCountry) => "Search country",
        (Language::English, LocationSearchCity) => "Search city",
        (Language::English, LocationSearchDistrict) => "Search district",
        (Language::English, LocationLoading) => "Loading...",
        (Language::English, LocationNoResults) => "No results found",
        (Language::English, LocationError) => "Could not load location data",
        (Language::English, LocationSaving) => "Updating prayer times...",
        (Language::English, LocationSaved) => "Location saved",
        (Language::English, LocationMore) => "Refine your search",
        (Language::English, WifiSetupTitle) => "WiFi Setup",
        (Language::English, WifiScanning) => "Scanning for networks...",
        (Language::English, WifiSelectNetwork) => "Select your network",
        (Language::English, WifiEnterManually) => "Enter manually",
        (Language::English, WifiRescan) => "Rescan",
        (Language::English, WifiNoNetworks) => "No networks found",
        (Language::English, WifiEnterSsid) => "Network name",
        (Language::English, WifiEnterPassword) => "Password",
        (Language::English, WifiReconnecting) => "Reconnecting...",
        (Language::English, WifiPasswordTooShort) => "Password too short (min 8)",
        (Language::English, KeySpace) => "space",
        (Language::English, KeyDone) => "OK",
        (Language::English, CalTitle) => "Touch Calibration",
        (Language::English, CalTapCrosshair) => "Tap the crosshair with a stylus",
        (Language::English, CalComplete) => "Calibration complete",
        (Language::English, CalFailed) => "Calibration failed",
        (Language::English, CalRetry) => "Please retry",
        (Language::English, CalSkipped) => "Calibration skipped",
        (Language::English, CalUsingDefaults) => "Using default values",
        (Language::English, CalRecalibrating) => "Recalibrating touch",

        // --- Nederlands ---
        (Language::Nederlands, AppTitle) => "Gebedstijden",
        (Language::Nederlands, Starting) => "Bezig met opstarten...",
        (Language::Nederlands, WifiConnecting) => "Verbinden met wifi...",
        (Language::Nederlands, WifiConnectFailed) => "Wifi-verbinding mislukt",
        (Language::Nederlands, Restarting) => "Opnieuw opstarten...",
        (Language::Nederlands, TimeSyncing) => "Klok synchroniseren...",
        (Language::Nederlands, PrayerDownloading) => "Gebedstijden downloaden...",
        (Language::Nederlands, PrayerFetchFailed) => "Kon gebedstijden niet ophalen",
        (Language::Nederlands, RetryingInBackground) => "Opnieuw proberen op de achtergrond...",
        (Language::Nederlands, PrayerDownloadFailed) => "Downloaden gebedstijden mislukt",
        (Language::Nederlands, Retrying) => "Opnieuw proberen...",
        (Language::Nederlands, PrayerDataMissing) => "Gebedsgegevens ontbreken, vernieuwen...",
        (Language::Nederlands, NextPrayer) => "VOLGEND GEBED:",
        (Language::Nederlands, SettingsTitle) => "Instellingen",
        (Language::Nederlands, LanguageHeading) => "Taal",
        (Language::Nederlands, DateHeading) => "Datum",
        (Language::Nederlands, DateMiladi) => "Gregoriaans",
        (Language::Nederlands, DateHijri) => "Hidjri",
        (Language::Nederlands, SystemHeading) => "Systeem",
        (Language::Nederlands, WifiMenu) => "Wifi",
        (Language::Nederlands, RecalibrateTouch) => "Kalibreren",
        (Language::Nederlands, AboutMenu) => "Over",
        (Language::Nederlands, LocationMenu) => "Locatie",
        (Language::Nederlands, AboutTitle) => "Over",
        (Language::Nederlands, AboutHardware) => "Hardware",
        (Language::Nederlands, AboutFirmware) => "Firmware",
        (Language::Nederlands, AboutMac) => "MAC-adres",
        (Language::Nederlands, LocationTitle) => "Locatie",
        (Language::Nederlands, LocationSearchCountry) => "Zoek land",
        (Language::Nederlands, LocationSearchCity) => "Zoek stad",
        (Language::Nederlands, LocationSearchDistrict) => "Zoek plaats",
        (Language::Nederlands, LocationLoading) => "Laden...",
        (Language::Nederlands, LocationNoResults) => "Geen resultaten",
        (Language::Nederlands, LocationError) => "Kon locatiegegevens niet laden",
        (Language::Nederlands, LocationSaving) => "Gebedstijden bijwerken...",
        (Language::Nederlands, LocationSaved) => "Locatie opgeslagen",
        (Language::Nederlands, LocationMore) => "Verfijn uw zoekopdracht",
        (Language::Nederlands, WifiSetupTitle) => "Wifi-instellingen",
        (Language::Nederlands, WifiScanning) => "Netwerken zoeken...",
        (Language::Nederlands, WifiSelectNetwork) => "Selecteer uw netwerk",
        (Language::Nederlands, WifiEnterManually) => "Handmatig invoeren",
        (Language::Nederlands, WifiRescan) => "Opnieuw zoeken",
        (Language::Nederlands, WifiNoNetworks) => "Geen netwerken gevonden",
        (Language::Nederlands, WifiEnterSsid) => "Netwerknaam",
        (Language::Nederlands, WifiEnterPassword) => "Wachtwoord",
        (Language::Nederlands, WifiReconnecting) => "Opnieuw verbinden...",
        (Language::Nederlands, WifiPasswordTooShort) => "Wachtwoord te kort (min. 8)",
        (Language::Nederlands, KeySpace) => "spatie",
        (Language::Nederlands, KeyDone) => "OK",
        (Language::Nederlands, CalTitle) => "Aanraakkalibratie",
        (Language::Nederlands, CalTapCrosshair) => "Tik met een stylus op het kruis",
        (Language::Nederlands, CalComplete) => "Kalibratie voltooid",
        (Language::Nederlands, CalFailed) => "Kalibratie mislukt",
        (Language::Nederlands, CalRetry) => "Probeer opnieuw",
        (Language::Nederlands, CalSkipped) => "Kalibratie overgeslagen",
        (Language::Nederlands, CalUsingDefaults) => "Standaardwaarden gebruiken",
        (Language::Nederlands, CalRecalibrating) => "Aanraking opnieuw kalibreren",
    }
}

/// Endonym shown for each language option in the settings selector. Each name
/// is written in its own script regardless of the currently active language.
pub fn language_label(lang: Language) -> &'static str {
    match lang {
        Language::Turkish => "Türkçe",
        Language::English => "English",
        Language::Nederlands => "Nederlands",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn u8_round_trip_is_stable() {
        for lang in Language::ALL {
            assert_eq!(Language::from_u8(lang.to_u8()), lang);
        }
    }

    #[test]
    fn unknown_byte_falls_back_to_turkish() {
        assert_eq!(Language::from_u8(99), Language::Turkish);
        assert_eq!(Language::default(), Language::Turkish);
    }

    #[test]
    fn retired_arabic_byte_falls_back_to_turkish() {
        assert_eq!(Language::from_u8(2), Language::Turkish);
        assert!(Language::ALL.iter().all(|l| l.to_u8() != 2));
    }

    #[test]
    fn every_language_has_five_prayer_and_seven_weekday_names() {
        for lang in Language::ALL {
            assert_eq!(prayer_names(lang).len(), 5);
            assert_eq!(weekday_names(lang).len(), 7);
            for name in prayer_names(lang) {
                assert!(!name.is_empty());
            }
        }
    }

    #[test]
    fn turkish_prayer_names_match_legacy_hardcoded_values() {
        assert_eq!(
            prayer_names(Language::Turkish),
            ["İMSAK", "ÖĞLE", "İKİNDİ", "AKŞAM", "YATSI"]
        );
    }

    #[test]
    fn every_message_resolves_for_every_language() {
        let msgs = [
            Msg::AppTitle,
            Msg::Starting,
            Msg::WifiConnecting,
            Msg::WifiConnectFailed,
            Msg::Restarting,
            Msg::TimeSyncing,
            Msg::PrayerDownloading,
            Msg::PrayerFetchFailed,
            Msg::RetryingInBackground,
            Msg::PrayerDownloadFailed,
            Msg::Retrying,
            Msg::PrayerDataMissing,
            Msg::NextPrayer,
            Msg::SettingsTitle,
            Msg::LanguageHeading,
            Msg::DateHeading,
            Msg::DateMiladi,
            Msg::DateHijri,
            Msg::SystemHeading,
            Msg::WifiMenu,
            Msg::RecalibrateTouch,
            Msg::AboutMenu,
            Msg::LocationMenu,
            Msg::AboutTitle,
            Msg::AboutHardware,
            Msg::AboutFirmware,
            Msg::AboutMac,
            Msg::LocationTitle,
            Msg::LocationSearchCountry,
            Msg::LocationSearchCity,
            Msg::LocationSearchDistrict,
            Msg::LocationLoading,
            Msg::LocationNoResults,
            Msg::LocationError,
            Msg::LocationSaving,
            Msg::LocationSaved,
            Msg::LocationMore,
            Msg::WifiSetupTitle,
            Msg::WifiScanning,
            Msg::WifiSelectNetwork,
            Msg::WifiEnterManually,
            Msg::WifiRescan,
            Msg::WifiNoNetworks,
            Msg::WifiEnterSsid,
            Msg::WifiEnterPassword,
            Msg::WifiReconnecting,
            Msg::WifiPasswordTooShort,
            Msg::KeySpace,
            Msg::KeyDone,
            Msg::CalTitle,
            Msg::CalTapCrosshair,
            Msg::CalComplete,
            Msg::CalFailed,
            Msg::CalRetry,
            Msg::CalSkipped,
            Msg::CalUsingDefaults,
            Msg::CalRecalibrating,
        ];
        for lang in Language::ALL {
            for &m in &msgs {
                assert!(!text(lang, m).is_empty(), "empty {:?} for {:?}", m, lang);
            }
        }
    }
}
