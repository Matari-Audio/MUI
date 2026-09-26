//! Material Symbols codepoints, for `mui_scene::icon`: `sym::HOME` at
//! compile time, [`codepoint`] for a name that arrives at run time.
//!
//! Generated from the `MaterialSymbolsOutlined[FILL,GRAD,opsz,wght].codepoints`
//! file that ships next to the font in google/material-design-icons. Regenerate
//! with `python3 tools/material_symbols_table.py <that file>`, which rewrites
//! everything below this header.
#![forbid(unsafe_code)]

/// The codepoint Material Symbols draws `name` at: `codepoint("home")` is
/// `Some(sym::HOME)`. `None` for a name the font does not have.
///
/// ```
/// use mui_symbols::{codepoint, sym};
/// assert_eq!(codepoint("home"), Some(sym::HOME));
/// assert_eq!(codepoint("no_such_icon"), None);
/// ```
pub fn codepoint(name: &str) -> Option<char> {
    TABLE
        .binary_search_by(|(n, _)| n.cmp(&name))
        .ok()
        .map(|i| TABLE[i].1)
}

#[cfg(test)]
#[test]
fn the_table_is_sorted_for_binary_search() {
    assert!(TABLE.windows(2).all(|w| w[0].0 < w[1].0));
}

#[rustfmt::skip]
/// One `char` per symbol: `sym::HOME`, `sym::_10K`.
pub mod sym {
    pub const _10K: char = '\u{E951}';
    pub const _10MP: char = '\u{E952}';
    pub const _11MP: char = '\u{E953}';
    pub const _123: char = '\u{EB8D}';
    pub const _12MP: char = '\u{E954}';
    pub const _13MP: char = '\u{E955}';
    pub const _14MP: char = '\u{E956}';
    pub const _15MP: char = '\u{E957}';
    pub const _16MP: char = '\u{E958}';
    pub const _17MP: char = '\u{E959}';
    pub const _18_UP_RATING: char = '\u{F8FD}';
    pub const _18MP: char = '\u{E95A}';
    pub const _19MP: char = '\u{E95B}';
    pub const _1K: char = '\u{E95C}';
    pub const _1K_PLUS: char = '\u{E95D}';
    pub const _1X_MOBILEDATA: char = '\u{EFCD}';
    pub const _1X_MOBILEDATA_BADGE: char = '\u{F7F1}';
    pub const _20MP: char = '\u{E95E}';
    pub const _21MP: char = '\u{E95F}';
    pub const _22MP: char = '\u{E960}';
    pub const _23MP: char = '\u{E961}';
    pub const _24FPS_SELECT: char = '\u{F3F2}';
    pub const _24MP: char = '\u{E962}';
    pub const _2D: char = '\u{EF37}';
    pub const _2D_2: char = '\u{FFF0E}';
    pub const _2K: char = '\u{E963}';
    pub const _2K_PLUS: char = '\u{E964}';
    pub const _2MP: char = '\u{E965}';
    pub const _30FPS: char = '\u{EFCE}';
    pub const _30FPS_SELECT: char = '\u{EFCF}';
    pub const _360: char = '\u{E577}';
    pub const _3D: char = '\u{ED38}';
    pub const _3D_2: char = '\u{FFF0F}';
    pub const _3D_ROTATION: char = '\u{E84D}';
    pub const _3G_MOBILEDATA: char = '\u{EFD0}';
    pub const _3G_MOBILEDATA_BADGE: char = '\u{F7F0}';
    pub const _3K: char = '\u{E966}';
    pub const _3K_PLUS: char = '\u{E967}';
    pub const _3MP: char = '\u{E968}';
    pub const _3P: char = '\u{EFD1}';
    pub const _4G_MOBILEDATA: char = '\u{EFD2}';
    pub const _4G_MOBILEDATA_BADGE: char = '\u{F7EF}';
    pub const _4G_PLUS_MOBILEDATA: char = '\u{EFD3}';
    pub const _4K: char = '\u{E072}';
    pub const _4K_PLUS: char = '\u{E969}';
    pub const _4MP: char = '\u{E96A}';
    pub const _50MP: char = '\u{F6F3}';
    pub const _5G: char = '\u{EF38}';
    pub const _5G_MOBILEDATA_BADGE: char = '\u{F7EE}';
    pub const _5K: char = '\u{E96B}';
    pub const _5K_PLUS: char = '\u{E96C}';
    pub const _5MP: char = '\u{E96D}';
    pub const _60FPS: char = '\u{EFD4}';
    pub const _60FPS_SELECT: char = '\u{EFD5}';
    pub const _6_FT_APART: char = '\u{F21E}';
    pub const _6K: char = '\u{E96E}';
    pub const _6K_PLUS: char = '\u{E96F}';
    pub const _6MP: char = '\u{E970}';
    pub const _7K: char = '\u{E971}';
    pub const _7K_PLUS: char = '\u{E972}';
    pub const _7MP: char = '\u{E973}';
    pub const _8K: char = '\u{E974}';
    pub const _8K_PLUS: char = '\u{E975}';
    pub const _8MP: char = '\u{E976}';
    pub const _9K: char = '\u{E977}';
    pub const _9K_PLUS: char = '\u{E978}';
    pub const _9MP: char = '\u{E979}';
    pub const ABC: char = '\u{EB94}';
    pub const AC_UNIT: char = '\u{EB3B}';
    pub const ACCESS_ALARM: char = '\u{E855}';
    pub const ACCESS_ALARMS: char = '\u{E855}';
    pub const ACCESS_TIME: char = '\u{EFD6}';
    pub const ACCESS_TIME_FILLED: char = '\u{EFD6}';
    pub const ACCESSIBILITY: char = '\u{E84E}';
    pub const ACCESSIBILITY_NEW: char = '\u{E92C}';
    pub const ACCESSIBLE: char = '\u{E914}';
    pub const ACCESSIBLE_FORWARD: char = '\u{E934}';
    pub const ACCESSIBLE_MENU: char = '\u{F34E}';
    pub const ACCOUNT_BALANCE: char = '\u{E84F}';
    pub const ACCOUNT_BALANCE_WALLET: char = '\u{E850}';
    pub const ACCOUNT_BOX: char = '\u{E851}';
    pub const ACCOUNT_CHILD: char = '\u{E852}';
    pub const ACCOUNT_CHILD_INVERT: char = '\u{E659}';
    pub const ACCOUNT_CIRCLE: char = '\u{F20B}';
    pub const ACCOUNT_CIRCLE_FILLED: char = '\u{F20B}';
    pub const ACCOUNT_CIRCLE_OFF: char = '\u{F7B3}';
    pub const ACCOUNT_TREE: char = '\u{E97A}';
    pub const ACTION_KEY: char = '\u{F502}';
    pub const ACTIVITY_ZONE: char = '\u{E1E6}';
    pub const ACUPUNCTURE: char = '\u{F2C4}';
    pub const ACUTE: char = '\u{E4CB}';
    pub const AD: char = '\u{E65A}';
    pub const AD_GROUP: char = '\u{E65B}';
    pub const AD_GROUP_OFF: char = '\u{EAE5}';
    pub const AD_OFF: char = '\u{F7B2}';
    pub const AD_UNITS: char = '\u{F2EB}';
    pub const ADAPTIVE_AUDIO_MIC: char = '\u{F4CC}';
    pub const ADAPTIVE_AUDIO_MIC_OFF: char = '\u{F4CB}';
    pub const ADB: char = '\u{E60E}';
    pub const ADD: char = '\u{E145}';
    pub const ADD_2: char = '\u{F3DD}';
    pub const ADD_A_PHOTO: char = '\u{E439}';
    pub const ADD_AD: char = '\u{E72A}';
    pub const ADD_ALARM: char = '\u{E856}';
    pub const ADD_ALERT: char = '\u{E003}';
    pub const ADD_BOX: char = '\u{E146}';
    pub const ADD_BUSINESS: char = '\u{E729}';
    pub const ADD_CALL: char = '\u{F0B7}';
    pub const ADD_CARD: char = '\u{EB86}';
    pub const ADD_CHART: char = '\u{EF3C}';
    pub const ADD_CIRCLE: char = '\u{E990}';
    pub const ADD_CIRCLE_OUTLINE: char = '\u{E990}';
    pub const ADD_COLUMN_LEFT: char = '\u{F425}';
    pub const ADD_COLUMN_RIGHT: char = '\u{F424}';
    pub const ADD_COMMENT: char = '\u{E266}';
    pub const ADD_DIAMOND: char = '\u{F49C}';
    pub const ADD_HOME: char = '\u{F8EB}';
    pub const ADD_HOME_WORK: char = '\u{F8ED}';
    pub const ADD_IC_CALL: char = '\u{F0B7}';
    pub const ADD_LINK: char = '\u{E178}';
    pub const ADD_LOCATION: char = '\u{E567}';
    pub const ADD_LOCATION_ALT: char = '\u{EF3A}';
    pub const ADD_MODERATOR: char = '\u{E97D}';
    pub const ADD_NOTES: char = '\u{E091}';
    pub const ADD_PHOTO_ALTERNATE: char = '\u{E43E}';
    pub const ADD_REACTION: char = '\u{E1D3}';
    pub const ADD_ROAD: char = '\u{EF3B}';
    pub const ADD_ROW_ABOVE: char = '\u{F423}';
    pub const ADD_ROW_BELOW: char = '\u{F422}';
    pub const ADD_SHOPPING_CART: char = '\u{E854}';
    pub const ADD_TASK: char = '\u{F23A}';
    pub const ADD_TO_DRIVE: char = '\u{E65C}';
    pub const ADD_TO_HOME_SCREEN: char = '\u{F2B9}';
    pub const ADD_TO_PHOTOS: char = '\u{E39D}';
    pub const ADD_TO_QUEUE: char = '\u{E05C}';
    pub const ADD_TRIANGLE: char = '\u{F48E}';
    pub const ADDCHART: char = '\u{EF3C}';
    pub const ADF_SCANNER: char = '\u{EADA}';
    pub const ADJUST: char = '\u{E39E}';
    pub const ADMIN_MEDS: char = '\u{E48D}';
    pub const ADMIN_PANEL_SETTINGS: char = '\u{EF3D}';
    pub const ADS_CLICK: char = '\u{E762}';
    pub const AGENDER: char = '\u{F888}';
    pub const AGRICULTURE: char = '\u{EA79}';
    pub const AIR: char = '\u{EFD8}';
    pub const AIR_FRESHENER: char = '\u{E2CA}';
    pub const AIR_PURIFIER: char = '\u{E97E}';
    pub const AIR_PURIFIER_GEN: char = '\u{E829}';
    pub const AIRLINE_SEAT_FLAT: char = '\u{E630}';
    pub const AIRLINE_SEAT_FLAT_ANGLED: char = '\u{E631}';
    pub const AIRLINE_SEAT_INDIVIDUAL_SUITE: char = '\u{E632}';
    pub const AIRLINE_SEAT_LEGROOM_EXTRA: char = '\u{E633}';
    pub const AIRLINE_SEAT_LEGROOM_NORMAL: char = '\u{E634}';
    pub const AIRLINE_SEAT_LEGROOM_REDUCED: char = '\u{E635}';
    pub const AIRLINE_SEAT_RECLINE_EXTRA: char = '\u{E636}';
    pub const AIRLINE_SEAT_RECLINE_NORMAL: char = '\u{E637}';
    pub const AIRLINE_STOPS: char = '\u{E7D0}';
    pub const AIRLINES: char = '\u{E7CA}';
    pub const AIRPLANE_TICKET: char = '\u{EFD9}';
    pub const AIRPLANEMODE_ACTIVE: char = '\u{E53D}';
    pub const AIRPLANEMODE_INACTIVE: char = '\u{E194}';
    pub const AIRPLAY: char = '\u{E055}';
    pub const AIRPORT_SHUTTLE: char = '\u{EB3C}';
    pub const AIRWARE: char = '\u{F154}';
    pub const AIRWAVE: char = '\u{F154}';
    pub const ALARM: char = '\u{E855}';
    pub const ALARM_ADD: char = '\u{E856}';
    pub const ALARM_OFF: char = '\u{E857}';
    pub const ALARM_ON: char = '\u{E858}';
    pub const ALARM_PAUSE: char = '\u{F35B}';
    pub const ALARM_SMART_WAKE: char = '\u{F6B0}';
    pub const ALBUM: char = '\u{E019}';
    pub const ALIGN_CENTER: char = '\u{E356}';
    pub const ALIGN_END: char = '\u{F797}';
    pub const ALIGN_FLEX_CENTER: char = '\u{F796}';
    pub const ALIGN_FLEX_END: char = '\u{F795}';
    pub const ALIGN_FLEX_START: char = '\u{F794}';
    pub const ALIGN_HORIZONTAL_CENTER: char = '\u{E00F}';
    pub const ALIGN_HORIZONTAL_LEFT: char = '\u{E00D}';
    pub const ALIGN_HORIZONTAL_RIGHT: char = '\u{E010}';
    pub const ALIGN_ITEMS_STRETCH: char = '\u{F793}';
    pub const ALIGN_JUSTIFY_CENTER: char = '\u{F792}';
    pub const ALIGN_JUSTIFY_FLEX_END: char = '\u{F791}';
    pub const ALIGN_JUSTIFY_FLEX_START: char = '\u{F790}';
    pub const ALIGN_JUSTIFY_SPACE_AROUND: char = '\u{F78F}';
    pub const ALIGN_JUSTIFY_SPACE_BETWEEN: char = '\u{F78E}';
    pub const ALIGN_JUSTIFY_SPACE_EVEN: char = '\u{F78D}';
    pub const ALIGN_JUSTIFY_STRETCH: char = '\u{F78C}';
    pub const ALIGN_SELF_STRETCH: char = '\u{F78B}';
    pub const ALIGN_SPACE_AROUND: char = '\u{F78A}';
    pub const ALIGN_SPACE_BETWEEN: char = '\u{F789}';
    pub const ALIGN_SPACE_EVEN: char = '\u{F788}';
    pub const ALIGN_START: char = '\u{F787}';
    pub const ALIGN_STRETCH: char = '\u{F786}';
    pub const ALIGN_VERTICAL_BOTTOM: char = '\u{E015}';
    pub const ALIGN_VERTICAL_CENTER: char = '\u{E011}';
    pub const ALIGN_VERTICAL_TOP: char = '\u{E00C}';
    pub const ALL_INBOX: char = '\u{E97F}';
    pub const ALL_INCLUSIVE: char = '\u{EB3D}';
    pub const ALL_MATCH: char = '\u{E093}';
    pub const ALL_OUT: char = '\u{E90B}';
    pub const ALLERGIES: char = '\u{E094}';
    pub const ALLERGY: char = '\u{E64E}';
    pub const ALT_ROUTE: char = '\u{F184}';
    pub const ALTERNATE_EMAIL: char = '\u{E0E6}';
    pub const ALTITUDE: char = '\u{F873}';
    pub const AMBIENT_SCREEN: char = '\u{F6C4}';
    pub const AMBULANCE: char = '\u{F803}';
    pub const AMEND: char = '\u{F802}';
    pub const AMP_STORIES: char = '\u{EA13}';
    pub const ANALYTICS: char = '\u{EF3E}';
    pub const ANCHOR: char = '\u{F1CD}';
    pub const ANDROID: char = '\u{E859}';
    pub const ANDROID_CELL_4_BAR: char = '\u{EF06}';
    pub const ANDROID_CELL_4_BAR_ALERT: char = '\u{EF09}';
    pub const ANDROID_CELL_4_BAR_OFF: char = '\u{EF08}';
    pub const ANDROID_CELL_4_BAR_PLUS: char = '\u{EF07}';
    pub const ANDROID_CELL_5_BAR: char = '\u{EF02}';
    pub const ANDROID_CELL_5_BAR_ALERT: char = '\u{EF05}';
    pub const ANDROID_CELL_5_BAR_OFF: char = '\u{EF04}';
    pub const ANDROID_CELL_5_BAR_PLUS: char = '\u{EF03}';
    pub const ANDROID_CELL_DUAL_4_BAR: char = '\u{EF0D}';
    pub const ANDROID_CELL_DUAL_4_BAR_ALERT: char = '\u{EF0F}';
    pub const ANDROID_CELL_DUAL_4_BAR_PLUS: char = '\u{EF0E}';
    pub const ANDROID_CELL_DUAL_5_BAR: char = '\u{EF0A}';
    pub const ANDROID_CELL_DUAL_5_BAR_ALERT: char = '\u{EF0C}';
    pub const ANDROID_CELL_DUAL_5_BAR_PLUS: char = '\u{EF0B}';
    pub const ANDROID_WIFI_3_BAR: char = '\u{EF16}';
    pub const ANDROID_WIFI_3_BAR_ALERT: char = '\u{EF1B}';
    pub const ANDROID_WIFI_3_BAR_LOCK: char = '\u{EF1A}';
    pub const ANDROID_WIFI_3_BAR_OFF: char = '\u{EF19}';
    pub const ANDROID_WIFI_3_BAR_PLUS: char = '\u{EF18}';
    pub const ANDROID_WIFI_3_BAR_QUESTION: char = '\u{EF17}';
    pub const ANDROID_WIFI_4_BAR: char = '\u{EF10}';
    pub const ANDROID_WIFI_4_BAR_ALERT: char = '\u{EF15}';
    pub const ANDROID_WIFI_4_BAR_LOCK: char = '\u{EF14}';
    pub const ANDROID_WIFI_4_BAR_OFF: char = '\u{EF13}';
    pub const ANDROID_WIFI_4_BAR_PLUS: char = '\u{EF12}';
    pub const ANDROID_WIFI_4_BAR_QUESTION: char = '\u{EF11}';
    pub const ANIMATED_IMAGES: char = '\u{F49A}';
    pub const ANIMATION: char = '\u{E71C}';
    pub const ANNOUNCEMENT: char = '\u{E87F}';
    pub const ANTIGRAVITY: char = '\u{FFFD2}';
    pub const AOD: char = '\u{F2E6}';
    pub const AOD_TABLET: char = '\u{F89F}';
    pub const AOD_WATCH: char = '\u{F6AC}';
    pub const APARTMENT: char = '\u{EA40}';
    pub const API: char = '\u{F1B7}';
    pub const APK_DOCUMENT: char = '\u{F88E}';
    pub const APK_INSTALL: char = '\u{F88F}';
    pub const APP_BADGING: char = '\u{F72F}';
    pub const APP_BLOCKING: char = '\u{F2E5}';
    pub const APP_PROMO: char = '\u{F2CD}';
    pub const APP_REGISTRATION: char = '\u{EF40}';
    pub const APP_SETTINGS_ALT: char = '\u{F2D9}';
    pub const APP_SHORTCUT: char = '\u{F2DF}';
    pub const APPAREL: char = '\u{EF7B}';
    pub const APPROVAL: char = '\u{E982}';
    pub const APPROVAL_DELEGATION: char = '\u{F84A}';
    pub const APPROVAL_DELEGATION_OFF: char = '\u{F2C5}';
    pub const APPS: char = '\u{E5C3}';
    pub const APPS_OUTAGE: char = '\u{E7CC}';
    pub const AQ: char = '\u{F55A}';
    pub const AQ_INDOOR: char = '\u{F55B}';
    pub const AR_ON_YOU: char = '\u{EF7C}';
    pub const AR_STICKERS: char = '\u{E983}';
    pub const ARCHITECTURE: char = '\u{EA3B}';
    pub const ARCHIVE: char = '\u{E149}';
    pub const AREA_CHART: char = '\u{E770}';
    pub const ARMING_COUNTDOWN: char = '\u{E78A}';
    pub const ARROW_AND_EDGE: char = '\u{F5D7}';
    pub const ARROW_BACK: char = '\u{E5C4}';
    pub const ARROW_BACK_2: char = '\u{F43A}';
    pub const ARROW_BACK_IOS: char = '\u{E5E0}';
    pub const ARROW_BACK_IOS_NEW: char = '\u{E2EA}';
    pub const ARROW_CIRCLE_DOWN: char = '\u{F181}';
    pub const ARROW_CIRCLE_LEFT: char = '\u{EAA7}';
    pub const ARROW_CIRCLE_RIGHT: char = '\u{EAAA}';
    pub const ARROW_CIRCLE_UP: char = '\u{F182}';
    pub const ARROW_COOL_DOWN: char = '\u{F4B6}';
    pub const ARROW_DOWNWARD: char = '\u{E5DB}';
    pub const ARROW_DOWNWARD_ALT: char = '\u{E984}';
    pub const ARROW_DROP_DOWN: char = '\u{E5C5}';
    pub const ARROW_DROP_DOWN_CIRCLE: char = '\u{E5C6}';
    pub const ARROW_DROP_UP: char = '\u{E5C7}';
    pub const ARROW_FORWARD: char = '\u{E5C8}';
    pub const ARROW_FORWARD_IOS: char = '\u{E5E1}';
    pub const ARROW_INSERT: char = '\u{F837}';
    pub const ARROW_LEFT: char = '\u{E5DE}';
    pub const ARROW_LEFT_ALT: char = '\u{EF7D}';
    pub const ARROW_MENU_CLOSE: char = '\u{F3D3}';
    pub const ARROW_MENU_OPEN: char = '\u{F3D2}';
    pub const ARROW_OR_EDGE: char = '\u{F5D6}';
    pub const ARROW_OUTWARD: char = '\u{F8CE}';
    pub const ARROW_RANGE: char = '\u{F69B}';
    pub const ARROW_RIGHT: char = '\u{E5DF}';
    pub const ARROW_RIGHT_ALT: char = '\u{E941}';
    pub const ARROW_SELECTOR_TOOL: char = '\u{F82F}';
    pub const ARROW_SHAPE_UP: char = '\u{EEF6}';
    pub const ARROW_SHAPE_UP_STACK: char = '\u{EEF7}';
    pub const ARROW_SHAPE_UP_STACK_2: char = '\u{EEF8}';
    pub const ARROW_SPLIT: char = '\u{EA04}';
    pub const ARROW_TOP_LEFT: char = '\u{F72E}';
    pub const ARROW_TOP_RIGHT: char = '\u{F72D}';
    pub const ARROW_UPLOAD_PROGRESS: char = '\u{F3F4}';
    pub const ARROW_UPLOAD_READY: char = '\u{F3F5}';
    pub const ARROW_UPWARD: char = '\u{E5D8}';
    pub const ARROW_UPWARD_ALT: char = '\u{E986}';
    pub const ARROW_WARM_UP: char = '\u{F4B5}';
    pub const ARROWS_INPUT: char = '\u{F394}';
    pub const ARROWS_LEFT_RIGHT_CIRCLE: char = '\u{EEE4}';
    pub const ARROWS_MORE_DOWN: char = '\u{F8AB}';
    pub const ARROWS_MORE_UP: char = '\u{F8AC}';
    pub const ARROWS_OUTPUT: char = '\u{F393}';
    pub const ARROWS_OUTWARD: char = '\u{F72C}';
    pub const ARROWS_UP_DOWN_CIRCLE: char = '\u{EEE3}';
    pub const ART_TRACK: char = '\u{E060}';
    pub const ARTICLE: char = '\u{EF42}';
    pub const ARTICLE_PERSON: char = '\u{F368}';
    pub const ARTICLE_SHORTCUT: char = '\u{F587}';
    pub const ARTIST: char = '\u{E01A}';
    pub const ASPECT_RATIO: char = '\u{E85B}';
    pub const ASSESSMENT: char = '\u{F0CC}';
    pub const ASSIGNMENT: char = '\u{E85D}';
    pub const ASSIGNMENT_ADD: char = '\u{F848}';
    pub const ASSIGNMENT_GLOBE: char = '\u{EEEC}';
    pub const ASSIGNMENT_IND: char = '\u{E85E}';
    pub const ASSIGNMENT_LATE: char = '\u{E85F}';
    pub const ASSIGNMENT_RETURN: char = '\u{E860}';
    pub const ASSIGNMENT_RETURNED: char = '\u{E861}';
    pub const ASSIGNMENT_TURNED_IN: char = '\u{E862}';
    pub const ASSIST_WALKER: char = '\u{F8D5}';
    pub const ASSISTANT: char = '\u{E39F}';
    pub const ASSISTANT_DEVICE: char = '\u{E987}';
    pub const ASSISTANT_DIRECTION: char = '\u{E988}';
    pub const ASSISTANT_NAVIGATION: char = '\u{E989}';
    pub const ASSISTANT_ON_HUB: char = '\u{F6C1}';
    pub const ASSISTANT_PHOTO: char = '\u{F0C6}';
    pub const ASSURED_WORKLOAD: char = '\u{EB6F}';
    pub const ASTERISK: char = '\u{F525}';
    pub const ASTROPHOTOGRAPHY_AUTO: char = '\u{F1D9}';
    pub const ASTROPHOTOGRAPHY_OFF: char = '\u{F1DA}';
    pub const ATM: char = '\u{E573}';
    pub const ATR: char = '\u{EBC7}';
    pub const ATTACH_EMAIL: char = '\u{EA5E}';
    pub const ATTACH_FILE: char = '\u{E226}';
    pub const ATTACH_FILE_ADD: char = '\u{F841}';
    pub const ATTACH_FILE_OFF: char = '\u{F4D9}';
    pub const ATTACH_MONEY: char = '\u{E227}';
    pub const ATTACHMENT: char = '\u{E2BC}';
    pub const ATTRACTIONS: char = '\u{EA52}';
    pub const ATTRIBUTION: char = '\u{EFDB}';
    pub const AUDIO_CAPTURE: char = '\u{FFF03}';
    pub const AUDIO_DESCRIPTION: char = '\u{F58C}';
    pub const AUDIO_FILE: char = '\u{EB82}';
    pub const AUDIO_VIDEO_RECEIVER: char = '\u{F5D3}';
    pub const AUDIOTRACK: char = '\u{E405}';
    pub const AUTO_ACTIVITY_ZONE: char = '\u{F8AD}';
    pub const AUTO_AWESOME: char = '\u{E65F}';
    pub const AUTO_AWESOME_MOSAIC: char = '\u{E660}';
    pub const AUTO_AWESOME_MOTION: char = '\u{E661}';
    pub const AUTO_DELETE: char = '\u{EA4C}';
    pub const AUTO_DETECT_VOICE: char = '\u{F83E}';
    pub const AUTO_DRAW_SOLID: char = '\u{E98A}';
    pub const AUTO_FIX: char = '\u{E663}';
    pub const AUTO_FIX_HIGH: char = '\u{E663}';
    pub const AUTO_FIX_NORMAL: char = '\u{E664}';
    pub const AUTO_FIX_OFF: char = '\u{E665}';
    pub const AUTO_GRAPH: char = '\u{E4FB}';
    pub const AUTO_LABEL: char = '\u{F6BE}';
    pub const AUTO_MEETING_ROOM: char = '\u{F6BF}';
    pub const AUTO_MODE: char = '\u{EC20}';
    pub const AUTO_READ_PAUSE: char = '\u{F219}';
    pub const AUTO_READ_PLAY: char = '\u{F216}';
    pub const AUTO_SCHEDULE: char = '\u{E214}';
    pub const AUTO_STORIES: char = '\u{E666}';
    pub const AUTO_STORIES_OFF: char = '\u{F267}';
    pub const AUTO_TIMER: char = '\u{EF7F}';
    pub const AUTO_TOWING: char = '\u{E71E}';
    pub const AUTO_TRANSMISSION: char = '\u{F53F}';
    pub const AUTO_VIDEOCAM: char = '\u{F6C0}';
    pub const AUTOFPS_SELECT: char = '\u{EFDC}';
    pub const AUTOMATION: char = '\u{F421}';
    pub const AUTOPAUSE: char = '\u{F6B6}';
    pub const AUTOPAY: char = '\u{F84B}';
    pub const AUTOPLAY: char = '\u{F6B5}';
    pub const AUTORENEW: char = '\u{E863}';
    pub const AUTOSTOP: char = '\u{F682}';
    pub const AV1: char = '\u{F4B0}';
    pub const AV_TIMER: char = '\u{E01B}';
    pub const AVC: char = '\u{F4AF}';
    pub const AVG_PACE: char = '\u{F6BB}';
    pub const AVG_TIME: char = '\u{F813}';
    pub const AVOCADO_BEAN: char = '\u{FFFA7}';
    pub const AWARD_MEAL: char = '\u{F241}';
    pub const AWARD_STAR: char = '\u{F612}';
    pub const AZM: char = '\u{F6EC}';
    pub const B_CIRCLE: char = '\u{EEE2}';
    pub const BABY_CHANGING_STATION: char = '\u{F19B}';
    pub const BACK_HAND: char = '\u{E764}';
    pub const BACK_TO_TAB: char = '\u{F72B}';
    pub const BACKGROUND_DOT_LARGE: char = '\u{F79E}';
    pub const BACKGROUND_DOT_SMALL: char = '\u{F514}';
    pub const BACKGROUND_GRID_SMALL: char = '\u{F79D}';
    pub const BACKGROUND_REPLACE: char = '\u{F20A}';
    pub const BACKLIGHT_HIGH: char = '\u{F7ED}';
    pub const BACKLIGHT_HIGH_OFF: char = '\u{F4EF}';
    pub const BACKLIGHT_LOW: char = '\u{F7EC}';
    pub const BACKPACK: char = '\u{F19C}';
    pub const BACKSPACE: char = '\u{E14A}';
    pub const BACKUP: char = '\u{E864}';
    pub const BACKUP_TABLE: char = '\u{EF43}';
    pub const BADGE: char = '\u{EA67}';
    pub const BADGE_CRITICAL_BATTERY: char = '\u{F156}';
    pub const BADMINTON: char = '\u{F2A8}';
    pub const BAKERY_DINING: char = '\u{EA53}';
    pub const BALANCE: char = '\u{EAF6}';
    pub const BALCONY: char = '\u{E58F}';
    pub const BALLOT: char = '\u{E172}';
    pub const BAR_CHART: char = '\u{E26B}';
    pub const BAR_CHART_4_BARS: char = '\u{F681}';
    pub const BAR_CHART_OFF: char = '\u{F411}';
    pub const BARCODE: char = '\u{E70B}';
    pub const BARCODE_READER: char = '\u{F85C}';
    pub const BARCODE_SCANNER: char = '\u{E70C}';
    pub const BAREFOOT: char = '\u{F871}';
    pub const BATCH_PREDICTION: char = '\u{F0F5}';
    pub const BATH_BEDROCK: char = '\u{F286}';
    pub const BATH_OUTDOOR: char = '\u{F6FB}';
    pub const BATH_PRIVATE: char = '\u{F6FA}';
    pub const BATH_PUBLIC_LARGE: char = '\u{F6F9}';
    pub const BATH_SOAK: char = '\u{F2A0}';
    pub const BATHROOM: char = '\u{EFDD}';
    pub const BATHTUB: char = '\u{EA41}';
    pub const BATTERY_0_BAR: char = '\u{EBDC}';
    pub const BATTERY_1_BAR: char = '\u{F09C}';
    pub const BATTERY_20: char = '\u{F09C}';
    pub const BATTERY_2_BAR: char = '\u{F09D}';
    pub const BATTERY_30: char = '\u{F09D}';
    pub const BATTERY_3_BAR: char = '\u{F09E}';
    pub const BATTERY_4_BAR: char = '\u{F09F}';
    pub const BATTERY_50: char = '\u{F09E}';
    pub const BATTERY_5_BAR: char = '\u{F0A0}';
    pub const BATTERY_60: char = '\u{F09F}';
    pub const BATTERY_6_BAR: char = '\u{F0A1}';
    pub const BATTERY_80: char = '\u{F0A0}';
    pub const BATTERY_90: char = '\u{F0A1}';
    pub const BATTERY_ALERT: char = '\u{E19C}';
    pub const BATTERY_ANDROID_0: char = '\u{F30D}';
    pub const BATTERY_ANDROID_1: char = '\u{F30C}';
    pub const BATTERY_ANDROID_2: char = '\u{F30B}';
    pub const BATTERY_ANDROID_3: char = '\u{F30A}';
    pub const BATTERY_ANDROID_4: char = '\u{F309}';
    pub const BATTERY_ANDROID_5: char = '\u{F308}';
    pub const BATTERY_ANDROID_6: char = '\u{F307}';
    pub const BATTERY_ANDROID_ALERT: char = '\u{F306}';
    pub const BATTERY_ANDROID_BOLT: char = '\u{F305}';
    pub const BATTERY_ANDROID_FRAME_1: char = '\u{F257}';
    pub const BATTERY_ANDROID_FRAME_2: char = '\u{F256}';
    pub const BATTERY_ANDROID_FRAME_3: char = '\u{F255}';
    pub const BATTERY_ANDROID_FRAME_4: char = '\u{F254}';
    pub const BATTERY_ANDROID_FRAME_5: char = '\u{F253}';
    pub const BATTERY_ANDROID_FRAME_6: char = '\u{F252}';
    pub const BATTERY_ANDROID_FRAME_ALERT: char = '\u{F251}';
    pub const BATTERY_ANDROID_FRAME_BOLT: char = '\u{F250}';
    pub const BATTERY_ANDROID_FRAME_FULL: char = '\u{F24F}';
    pub const BATTERY_ANDROID_FRAME_PLUS: char = '\u{F24E}';
    pub const BATTERY_ANDROID_FRAME_QUESTION: char = '\u{F24D}';
    pub const BATTERY_ANDROID_FRAME_SHARE: char = '\u{F24C}';
    pub const BATTERY_ANDROID_FRAME_SHIELD: char = '\u{F24B}';
    pub const BATTERY_ANDROID_FULL: char = '\u{F304}';
    pub const BATTERY_ANDROID_PLUS: char = '\u{F303}';
    pub const BATTERY_ANDROID_QUESTION: char = '\u{F302}';
    pub const BATTERY_ANDROID_SHARE: char = '\u{F301}';
    pub const BATTERY_ANDROID_SHIELD: char = '\u{F300}';
    pub const BATTERY_CHANGE: char = '\u{F7EB}';
    pub const BATTERY_CHARGING_20: char = '\u{F0A2}';
    pub const BATTERY_CHARGING_20_2: char = '\u{FFF3E}';
    pub const BATTERY_CHARGING_30: char = '\u{F0A3}';
    pub const BATTERY_CHARGING_30_2: char = '\u{FFF3D}';
    pub const BATTERY_CHARGING_50: char = '\u{F0A4}';
    pub const BATTERY_CHARGING_50_2: char = '\u{FFF3C}';
    pub const BATTERY_CHARGING_60: char = '\u{F0A5}';
    pub const BATTERY_CHARGING_60_2: char = '\u{FFF3B}';
    pub const BATTERY_CHARGING_80: char = '\u{F0A6}';
    pub const BATTERY_CHARGING_80_2: char = '\u{FFF3A}';
    pub const BATTERY_CHARGING_90: char = '\u{F0A7}';
    pub const BATTERY_CHARGING_FULL: char = '\u{E1A3}';
    pub const BATTERY_CHARGING_FULL_2: char = '\u{FFF39}';
    pub const BATTERY_ERROR: char = '\u{F7EA}';
    pub const BATTERY_FULL: char = '\u{E1A5}';
    pub const BATTERY_FULL_ALT: char = '\u{F13B}';
    pub const BATTERY_HORIZ_000: char = '\u{F8AE}';
    pub const BATTERY_HORIZ_050: char = '\u{F8AF}';
    pub const BATTERY_HORIZ_075: char = '\u{F8B0}';
    pub const BATTERY_LOW: char = '\u{F155}';
    pub const BATTERY_PLUS: char = '\u{F7E9}';
    pub const BATTERY_PROFILE: char = '\u{E206}';
    pub const BATTERY_SAVER: char = '\u{EFDE}';
    pub const BATTERY_SHARE: char = '\u{F67E}';
    pub const BATTERY_STATUS_GOOD: char = '\u{F67D}';
    pub const BATTERY_STD: char = '\u{E1A5}';
    pub const BATTERY_UNKNOWN: char = '\u{E1A6}';
    pub const BATTERY_VERT_005: char = '\u{F8B1}';
    pub const BATTERY_VERT_020: char = '\u{F8B2}';
    pub const BATTERY_VERT_050: char = '\u{F8B3}';
    pub const BATTERY_VERY_LOW: char = '\u{F156}';
    pub const BEACH_ACCESS: char = '\u{EB3E}';
    pub const BED: char = '\u{EFDF}';
    pub const BEDROOM_BABY: char = '\u{EFE0}';
    pub const BEDROOM_CHILD: char = '\u{EFE1}';
    pub const BEDROOM_PARENT: char = '\u{EFE2}';
    pub const BEDTIME: char = '\u{F159}';
    pub const BEDTIME_OFF: char = '\u{EB76}';
    pub const BEENHERE: char = '\u{E52D}';
    pub const BEER_MEAL: char = '\u{F285}';
    pub const BENTO: char = '\u{F1F4}';
    pub const BIA: char = '\u{F6EB}';
    pub const BID_LANDSCAPE: char = '\u{E667}';
    pub const BID_LANDSCAPE_DISABLED: char = '\u{EF81}';
    pub const BIGTOP_UPDATES: char = '\u{E669}';
    pub const BIKE_DOCK: char = '\u{F47B}';
    pub const BIKE_LANE: char = '\u{F47A}';
    pub const BIKE_SCOOTER: char = '\u{EF45}';
    pub const BIOTECH: char = '\u{EA3A}';
    pub const BLANKET: char = '\u{E828}';
    pub const BLENDER: char = '\u{EFE3}';
    pub const BLIND: char = '\u{F8D6}';
    pub const BLINDS: char = '\u{E286}';
    pub const BLINDS_2: char = '\u{FFF78}';
    pub const BLINDS_2_CLOSED: char = '\u{FFF79}';
    pub const BLINDS_CLOSED: char = '\u{EC1F}';
    pub const BLOCK: char = '\u{F08C}';
    pub const BLOOD_PRESSURE: char = '\u{E097}';
    pub const BLOODTYPE: char = '\u{EFE4}';
    pub const BLUETOOTH: char = '\u{E1A7}';
    pub const BLUETOOTH_AUDIO: char = '\u{E60F}';
    pub const BLUETOOTH_CONNECTED: char = '\u{E1A8}';
    pub const BLUETOOTH_DISABLED: char = '\u{E1A9}';
    pub const BLUETOOTH_DRIVE: char = '\u{EFE5}';
    pub const BLUETOOTH_SEARCHING: char = '\u{E60F}';
    pub const BLUR_CIRCULAR: char = '\u{E3A2}';
    pub const BLUR_LINEAR: char = '\u{E3A3}';
    pub const BLUR_MEDIUM: char = '\u{E84C}';
    pub const BLUR_OFF: char = '\u{E3A4}';
    pub const BLUR_ON: char = '\u{E3A5}';
    pub const BLUR_SHORT: char = '\u{E8CF}';
    pub const BOAT_BUS: char = '\u{F36D}';
    pub const BOAT_RAILWAY: char = '\u{F36C}';
    pub const BODY_FAT: char = '\u{E098}';
    pub const BODY_SYSTEM: char = '\u{E099}';
    pub const BOLT: char = '\u{EA0B}';
    pub const BOLT_BOOST: char = '\u{FFF6A}';
    pub const BOMB: char = '\u{F568}';
    pub const BOOK: char = '\u{E86E}';
    pub const BOOK_2: char = '\u{F53E}';
    pub const BOOK_3: char = '\u{F53D}';
    pub const BOOK_4: char = '\u{F53C}';
    pub const BOOK_5: char = '\u{F53B}';
    pub const BOOK_6: char = '\u{F3DF}';
    pub const BOOK_ONLINE: char = '\u{F2E4}';
    pub const BOOK_RIBBON: char = '\u{F3E7}';
    pub const BOOKMARK: char = '\u{E8E7}';
    pub const BOOKMARK_ADD: char = '\u{E598}';
    pub const BOOKMARK_ADDED: char = '\u{E599}';
    pub const BOOKMARK_BAG: char = '\u{F410}';
    pub const BOOKMARK_BORDER: char = '\u{E8E7}';
    pub const BOOKMARK_CHECK: char = '\u{F457}';
    pub const BOOKMARK_FLAG: char = '\u{F456}';
    pub const BOOKMARK_HEART: char = '\u{F455}';
    pub const BOOKMARK_MANAGER: char = '\u{F7B1}';
    pub const BOOKMARK_REMOVE: char = '\u{E59A}';
    pub const BOOKMARK_STACKS: char = '\u{EEE8}';
    pub const BOOKMARK_STAR: char = '\u{F454}';
    pub const BOOKMARKS: char = '\u{E98B}';
    pub const BOOKS_MOVIES_AND_MUSIC: char = '\u{EF82}';
    pub const BORDER_ALL: char = '\u{E228}';
    pub const BORDER_BOTTOM: char = '\u{E229}';
    pub const BORDER_CLEAR: char = '\u{E22A}';
    pub const BORDER_COLOR: char = '\u{E22B}';
    pub const BORDER_HORIZONTAL: char = '\u{E22C}';
    pub const BORDER_INNER: char = '\u{E22D}';
    pub const BORDER_LEFT: char = '\u{E22E}';
    pub const BORDER_OUTER: char = '\u{E22F}';
    pub const BORDER_RIGHT: char = '\u{E230}';
    pub const BORDER_STYLE: char = '\u{E231}';
    pub const BORDER_TOP: char = '\u{E232}';
    pub const BORDER_VERTICAL: char = '\u{E233}';
    pub const BORG: char = '\u{F40D}';
    pub const BOTTOM_APP_BAR: char = '\u{E730}';
    pub const BOTTOM_DRAWER: char = '\u{E72D}';
    pub const BOTTOM_NAVIGATION: char = '\u{E98C}';
    pub const BOTTOM_PANEL_CLOSE: char = '\u{F72A}';
    pub const BOTTOM_PANEL_OPEN: char = '\u{F729}';
    pub const BOTTOM_RIGHT_CLICK: char = '\u{F684}';
    pub const BOTTOM_SHEETS: char = '\u{E98D}';
    pub const BOX: char = '\u{F5A4}';
    pub const BOX_ADD: char = '\u{F5A5}';
    pub const BOX_EDIT: char = '\u{F5A6}';
    pub const BOY: char = '\u{EB67}';
    pub const BRAND_AWARENESS: char = '\u{E98E}';
    pub const BRAND_FAMILY: char = '\u{F4F1}';
    pub const BRANDING_WATERMARK: char = '\u{E06B}';
    pub const BREAKFAST_DINING: char = '\u{EA54}';
    pub const BREAKING_NEWS: char = '\u{EA08}';
    pub const BREAKING_NEWS_ALT_1: char = '\u{F0BA}';
    pub const BREASTFEEDING: char = '\u{F856}';
    pub const BRICK: char = '\u{F388}';
    pub const BRIEFCASE_MEAL: char = '\u{F246}';
    pub const BRIGHTNESS_1: char = '\u{E3FA}';
    pub const BRIGHTNESS_2: char = '\u{F036}';
    pub const BRIGHTNESS_3: char = '\u{E3A8}';
    pub const BRIGHTNESS_4: char = '\u{E3A9}';
    pub const BRIGHTNESS_5: char = '\u{E3AA}';
    pub const BRIGHTNESS_6: char = '\u{E3AB}';
    pub const BRIGHTNESS_7: char = '\u{E3AC}';
    pub const BRIGHTNESS_ALERT: char = '\u{F5CF}';
    pub const BRIGHTNESS_AUTO: char = '\u{E1AB}';
    pub const BRIGHTNESS_EMPTY: char = '\u{F7E8}';
    pub const BRIGHTNESS_HIGH: char = '\u{E1AC}';
    pub const BRIGHTNESS_LOW: char = '\u{E1AD}';
    pub const BRIGHTNESS_MEDIUM: char = '\u{E1AE}';
    pub const BRING_YOUR_OWN_IP: char = '\u{E016}';
    pub const BROADCAST_ON_HOME: char = '\u{F8F8}';
    pub const BROADCAST_ON_PERSONAL: char = '\u{F8F9}';
    pub const BROKEN_IMAGE: char = '\u{E3AD}';
    pub const BROWSE: char = '\u{EB13}';
    pub const BROWSE_ACTIVITY: char = '\u{F8A5}';
    pub const BROWSE_GALLERY: char = '\u{EBD1}';
    pub const BROWSER_NOT_SUPPORTED: char = '\u{EF47}';
    pub const BROWSER_UPDATED: char = '\u{E7CF}';
    pub const BRUNCH_DINING: char = '\u{EA73}';
    pub const BRUSH: char = '\u{E3AE}';
    pub const BUBBLE: char = '\u{EF83}';
    pub const BUBBLE_CHART: char = '\u{E6DD}';
    pub const BUBBLES: char = '\u{F64E}';
    pub const BUCKET_CHECK: char = '\u{EF2A}';
    pub const BUG_REPORT: char = '\u{E868}';
    pub const BUILD: char = '\u{F8CD}';
    pub const BUILD_CIRCLE: char = '\u{EF48}';
    pub const BULLET_CHART: char = '\u{FFEC7}';
    pub const BUNGALOW: char = '\u{E591}';
    pub const BURST_MODE: char = '\u{E43C}';
    pub const BUS_ALERT: char = '\u{E98F}';
    pub const BUS_MAP_PIN: char = '\u{FFFA2}';
    pub const BUS_RAILWAY: char = '\u{F36B}';
    pub const BUSINESS: char = '\u{E7EE}';
    pub const BUSINESS_CENTER: char = '\u{EB3F}';
    pub const BUSINESS_CHIP: char = '\u{F84C}';
    pub const BUSINESS_MESSAGES: char = '\u{EF84}';
    pub const BUTTONS_ALT: char = '\u{E72F}';
    pub const CABIN: char = '\u{E589}';
    pub const CABLE: char = '\u{EFE6}';
    pub const CABLE_CAR: char = '\u{F479}';
    pub const CACHED: char = '\u{E86A}';
    pub const CADENCE: char = '\u{F4B4}';
    pub const CAKE: char = '\u{E7E9}';
    pub const CAKE_ADD: char = '\u{F85B}';
    pub const CALCULATE: char = '\u{EA5F}';
    pub const CALENDAR_ADD_ON: char = '\u{EF85}';
    pub const CALENDAR_APPS_SCRIPT: char = '\u{F0BB}';
    pub const CALENDAR_CHECK: char = '\u{F243}';
    pub const CALENDAR_CLOCK: char = '\u{F540}';
    pub const CALENDAR_LOCK: char = '\u{F242}';
    pub const CALENDAR_MEAL: char = '\u{F296}';
    pub const CALENDAR_MEAL_2: char = '\u{F240}';
    pub const CALENDAR_MONTH: char = '\u{EBCC}';
    pub const CALENDAR_TODAY: char = '\u{E935}';
    pub const CALENDAR_VIEW_DAY: char = '\u{E936}';
    pub const CALENDAR_VIEW_MONTH: char = '\u{EFE7}';
    pub const CALENDAR_VIEW_WEEK: char = '\u{EFE8}';
    pub const CALL: char = '\u{F0D4}';
    pub const CALL_END: char = '\u{F0BC}';
    pub const CALL_END_ALT: char = '\u{F0BC}';
    pub const CALL_LOG: char = '\u{E08E}';
    pub const CALL_MADE: char = '\u{E0B2}';
    pub const CALL_MERGE: char = '\u{E0B3}';
    pub const CALL_MISSED: char = '\u{E0B4}';
    pub const CALL_MISSED_OUTGOING: char = '\u{E0E4}';
    pub const CALL_QUALITY: char = '\u{F652}';
    pub const CALL_RECEIVED: char = '\u{E0B5}';
    pub const CALL_SPLIT: char = '\u{E0B6}';
    pub const CALL_TO_ACTION: char = '\u{E06C}';
    pub const CAMERA: char = '\u{E3AF}';
    pub const CAMERA_ALT: char = '\u{E412}';
    pub const CAMERA_ENHANCE: char = '\u{E8FC}';
    pub const CAMERA_FRONT: char = '\u{F2C9}';
    pub const CAMERA_INDOOR: char = '\u{EFE9}';
    pub const CAMERA_OUTDOOR: char = '\u{EFEA}';
    pub const CAMERA_REAR: char = '\u{F2C8}';
    pub const CAMERA_ROLL: char = '\u{E3B3}';
    pub const CAMERA_VIDEO: char = '\u{F7A6}';
    pub const CAMERASWITCH: char = '\u{EFEB}';
    pub const CAMPAIGN: char = '\u{EF49}';
    pub const CAMPING: char = '\u{F8A2}';
    pub const CANCEL: char = '\u{E888}';
    pub const CANCEL_PRESENTATION: char = '\u{E0E9}';
    pub const CANCEL_SCHEDULE_SEND: char = '\u{EA39}';
    pub const CANDLE: char = '\u{F588}';
    pub const CANDLESTICK_CHART: char = '\u{EAD4}';
    pub const CANNABIS: char = '\u{F2F3}';
    pub const CAPTIVE_PORTAL: char = '\u{F728}';
    pub const CAPTURE: char = '\u{F727}';
    pub const CAR_CRASH: char = '\u{EBF2}';
    pub const CAR_DEFROST_LEFT: char = '\u{F344}';
    pub const CAR_DEFROST_LOW_LEFT: char = '\u{F343}';
    pub const CAR_DEFROST_LOW_RIGHT: char = '\u{F342}';
    pub const CAR_DEFROST_MID_LEFT: char = '\u{F278}';
    pub const CAR_DEFROST_MID_LOW_LEFT: char = '\u{F341}';
    pub const CAR_DEFROST_MID_LOW_RIGHT: char = '\u{F277}';
    pub const CAR_DEFROST_MID_RIGHT: char = '\u{F340}';
    pub const CAR_DEFROST_RIGHT: char = '\u{F33F}';
    pub const CAR_FAN_LOW_LEFT: char = '\u{F33E}';
    pub const CAR_FAN_LOW_MID_LEFT: char = '\u{F33D}';
    pub const CAR_FAN_LOW_RIGHT: char = '\u{F33C}';
    pub const CAR_FAN_MID_LEFT: char = '\u{F33B}';
    pub const CAR_FAN_MID_LOW_RIGHT: char = '\u{F33A}';
    pub const CAR_FAN_MID_RIGHT: char = '\u{F339}';
    pub const CAR_FAN_RECIRCULATE: char = '\u{F338}';
    pub const CAR_FAN_RECIRCULATE_2: char = '\u{FFF40}';
    pub const CAR_GEAR: char = '\u{F337}';
    pub const CAR_LOCK: char = '\u{F336}';
    pub const CAR_MIRROR_HEAT: char = '\u{F335}';
    pub const CAR_RENTAL: char = '\u{EA55}';
    pub const CAR_REPAIR: char = '\u{EA56}';
    pub const CAR_SEAT_OFF: char = '\u{FFEBE}';
    pub const CAR_TAG: char = '\u{F4E3}';
    pub const CARD_GIFTCARD: char = '\u{E8F6}';
    pub const CARD_MEMBERSHIP: char = '\u{E8F7}';
    pub const CARD_TRAVEL: char = '\u{E8F8}';
    pub const CARDIO_LOAD: char = '\u{F4B9}';
    pub const CARDIOLOGY: char = '\u{E09C}';
    pub const CARDS: char = '\u{E991}';
    pub const CARDS_STACK: char = '\u{F38F}';
    pub const CARDS_STAR: char = '\u{F375}';
    pub const CARPENTER: char = '\u{F1F8}';
    pub const CARRY_ON_BAG: char = '\u{EB08}';
    pub const CARRY_ON_BAG_CHECKED: char = '\u{EB0B}';
    pub const CARRY_ON_BAG_INACTIVE: char = '\u{EB0A}';
    pub const CARRY_ON_BAG_QUESTION: char = '\u{EB09}';
    pub const CASES: char = '\u{E992}';
    pub const CASINO: char = '\u{EB40}';
    pub const CAST: char = '\u{E307}';
    pub const CAST_CONNECTED: char = '\u{E308}';
    pub const CAST_FOR_EDUCATION: char = '\u{EFEC}';
    pub const CAST_PAUSE: char = '\u{F5F0}';
    pub const CAST_WARNING: char = '\u{F5EF}';
    pub const CASTLE: char = '\u{EAB1}';
    pub const CATEGORY: char = '\u{E72C}';
    pub const CATEGORY_SEARCH: char = '\u{F437}';
    pub const CELEBRATION: char = '\u{EA65}';
    pub const CELL_MERGE: char = '\u{F82E}';
    pub const CELL_TOWER: char = '\u{EBBA}';
    pub const CELL_WIFI: char = '\u{E0EC}';
    pub const CENTER_FOCUS_STRONG: char = '\u{E3B4}';
    pub const CENTER_FOCUS_WEAK: char = '\u{E3B5}';
    pub const CHAIR: char = '\u{EFED}';
    pub const CHAIR_ALT: char = '\u{EFEE}';
    pub const CHAIR_COUNTER: char = '\u{F29F}';
    pub const CHAIR_FIREPLACE: char = '\u{F29E}';
    pub const CHAIR_UMBRELLA: char = '\u{F29D}';
    pub const CHALET: char = '\u{E585}';
    pub const CHANGE_CIRCLE: char = '\u{E2E7}';
    pub const CHANGE_HISTORY: char = '\u{E86B}';
    pub const CHARGER: char = '\u{E2AE}';
    pub const CHARGING_STATION: char = '\u{F2E3}';
    pub const CHART_DATA: char = '\u{E473}';
    pub const CHAT: char = '\u{E0C9}';
    pub const CHAT_ADD_ON: char = '\u{F0F3}';
    pub const CHAT_APPS_SCRIPT: char = '\u{F0BD}';
    pub const CHAT_BUBBLE: char = '\u{E0CB}';
    pub const CHAT_BUBBLE_OFF: char = '\u{FFFBB}';
    pub const CHAT_BUBBLE_OUTLINE: char = '\u{E0CB}';
    pub const CHAT_DASHED: char = '\u{EEED}';
    pub const CHAT_ERROR: char = '\u{F7AC}';
    pub const CHAT_INFO: char = '\u{F52B}';
    pub const CHAT_PASTE_GO: char = '\u{F6BD}';
    pub const CHAT_PASTE_GO_2: char = '\u{F3CB}';
    pub const CHECK: char = '\u{E668}';
    pub const CHECK_ALERT: char = '\u{FFF85}';
    pub const CHECK_BOX: char = '\u{E9DE}';
    pub const CHECK_BOX_OUTLINE_BLANK: char = '\u{E835}';
    pub const CHECK_CIRCLE: char = '\u{F0BE}';
    pub const CHECK_CIRCLE_FILLED: char = '\u{F0BE}';
    pub const CHECK_CIRCLE_OUTLINE: char = '\u{F0BE}';
    pub const CHECK_CIRCLE_UNREAD: char = '\u{F27E}';
    pub const CHECK_IN_OUT: char = '\u{F6F6}';
    pub const CHECK_INDETERMINATE_SMALL: char = '\u{F88A}';
    pub const CHECK_SMALL: char = '\u{F88B}';
    pub const CHECKBOOK: char = '\u{E70D}';
    pub const CHECKED_BAG: char = '\u{EB0C}';
    pub const CHECKED_BAG_QUESTION: char = '\u{EB0D}';
    pub const CHECKLIST: char = '\u{E6B1}';
    pub const CHECKLIST_RTL: char = '\u{E6B3}';
    pub const CHECKROOM: char = '\u{F19E}';
    pub const CHEER: char = '\u{F6A8}';
    pub const CHEF_HAT: char = '\u{F357}';
    pub const CHESS: char = '\u{F5E7}';
    pub const CHESS_BISHOP: char = '\u{F261}';
    pub const CHESS_BISHOP_2: char = '\u{F262}';
    pub const CHESS_KING: char = '\u{F25F}';
    pub const CHESS_KING_2: char = '\u{F260}';
    pub const CHESS_KNIGHT: char = '\u{F25E}';
    pub const CHESS_PAWN: char = '\u{F3B6}';
    pub const CHESS_PAWN_2: char = '\u{F25D}';
    pub const CHESS_QUEEN: char = '\u{F25C}';
    pub const CHESS_ROOK: char = '\u{F25B}';
    pub const CHEVRON_BACKWARD: char = '\u{F46B}';
    pub const CHEVRON_FORWARD: char = '\u{F46A}';
    pub const CHEVRON_LEFT: char = '\u{E5CB}';
    pub const CHEVRON_LINE_UP: char = '\u{EEC3}';
    pub const CHEVRON_RIGHT: char = '\u{E5CC}';
    pub const CHILD_CARE: char = '\u{EB41}';
    pub const CHILD_FRIENDLY: char = '\u{EF80}';
    pub const CHILD_HAT: char = '\u{EF30}';
    pub const CHIP_EXTRACTION: char = '\u{F821}';
    pub const CHIPS: char = '\u{E993}';
    pub const CHROME_READER_MODE: char = '\u{E86D}';
    pub const CHROMECAST_2: char = '\u{F17B}';
    pub const CHROMECAST_DEVICE: char = '\u{E83C}';
    pub const CHRONIC: char = '\u{EBB2}';
    pub const CHURCH: char = '\u{EAAE}';
    pub const CINEMATIC_BLUR: char = '\u{F853}';
    pub const CIRCLE: char = '\u{EF4A}';
    pub const CIRCLE_CIRCLE: char = '\u{EEE1}';
    pub const CIRCLE_NOTIFICATIONS: char = '\u{E994}';
    pub const CIRCLES: char = '\u{E7EA}';
    pub const CIRCLES_EXT: char = '\u{E7EC}';
    pub const CLARIFY: char = '\u{F0BF}';
    pub const CLASS: char = '\u{E86E}';
    pub const CLEAN_HANDS: char = '\u{F21F}';
    pub const CLEANING: char = '\u{E995}';
    pub const CLEANING_BUCKET: char = '\u{F8B4}';
    pub const CLEANING_SERVICES: char = '\u{F0FF}';
    pub const CLEAR: char = '\u{E5CD}';
    pub const CLEAR_ALL: char = '\u{E0B8}';
    pub const CLEAR_DAY: char = '\u{F157}';
    pub const CLEAR_NIGHT: char = '\u{F159}';
    pub const CLIMATE_MINI_SPLIT: char = '\u{F8B5}';
    pub const CLINICAL_NOTES: char = '\u{E09E}';
    pub const CLOCK_ARROW_DOWN: char = '\u{F382}';
    pub const CLOCK_ARROW_UP: char = '\u{F381}';
    pub const CLOCK_LOADER_10: char = '\u{F726}';
    pub const CLOCK_LOADER_20: char = '\u{F725}';
    pub const CLOCK_LOADER_40: char = '\u{F724}';
    pub const CLOCK_LOADER_60: char = '\u{F723}';
    pub const CLOCK_LOADER_80: char = '\u{F722}';
    pub const CLOCK_LOADER_90: char = '\u{F721}';
    pub const CLOSE: char = '\u{E5CD}';
    pub const CLOSE_FULLSCREEN: char = '\u{F1CF}';
    pub const CLOSE_SMALL: char = '\u{F508}';
    pub const CLOSED_CAPTION: char = '\u{E996}';
    pub const CLOSED_CAPTION_ADD: char = '\u{F4AE}';
    pub const CLOSED_CAPTION_DISABLED: char = '\u{F1DC}';
    pub const CLOSED_CAPTION_OFF: char = '\u{E996}';
    pub const CLOUD: char = '\u{F15C}';
    pub const CLOUD_ALERT: char = '\u{F3CC}';
    pub const CLOUD_CIRCLE: char = '\u{E2BE}';
    pub const CLOUD_DONE: char = '\u{E2BF}';
    pub const CLOUD_DOWNLOAD: char = '\u{E2C0}';
    pub const CLOUD_LOCK: char = '\u{F386}';
    pub const CLOUD_OFF: char = '\u{E2C1}';
    pub const CLOUD_QUEUE: char = '\u{F15C}';
    pub const CLOUD_SYNC: char = '\u{EB5A}';
    pub const CLOUD_UPLOAD: char = '\u{E2C3}';
    pub const CLOUDY: char = '\u{F15C}';
    pub const CLOUDY_FILLED: char = '\u{F15C}';
    pub const CLOUDY_SNOWING: char = '\u{E810}';
    pub const CO2: char = '\u{E7B0}';
    pub const CO_PRESENT: char = '\u{EAF0}';
    pub const CODE: char = '\u{E86F}';
    pub const CODE_BLOCKS: char = '\u{F84D}';
    pub const CODE_OFF: char = '\u{E4F3}';
    pub const CODE_XML: char = '\u{FFF8B}';
    pub const COFFEE: char = '\u{EFEF}';
    pub const COFFEE_MAKER: char = '\u{EFF0}';
    pub const COGNITION: char = '\u{E09F}';
    pub const COGNITION_2: char = '\u{F3B5}';
    pub const COLLAPSE_ALL: char = '\u{E944}';
    pub const COLLAPSE_CONTENT: char = '\u{F507}';
    pub const COLLECTIONS: char = '\u{E3D3}';
    pub const COLLECTIONS_BOOKMARK: char = '\u{E431}';
    pub const COLOR_LENS: char = '\u{E40A}';
    pub const COLORIZE: char = '\u{E3B8}';
    pub const COLORS: char = '\u{E997}';
    pub const COMBINE_COLUMNS: char = '\u{F420}';
    pub const COMEDY_MASK: char = '\u{F4D6}';
    pub const COMIC_BUBBLE: char = '\u{F5DD}';
    pub const COMMENT: char = '\u{E24C}';
    pub const COMMENT_BANK: char = '\u{EA4E}';
    pub const COMMENTS_DISABLED: char = '\u{E7A2}';
    pub const COMMIT: char = '\u{EAF5}';
    pub const COMMUNICATION: char = '\u{E27C}';
    pub const COMMUNITIES: char = '\u{EB16}';
    pub const COMMUNITIES_FILLED: char = '\u{EB16}';
    pub const COMMUTE: char = '\u{E940}';
    pub const COMPARE: char = '\u{E3B9}';
    pub const COMPARE_ARROWS: char = '\u{E915}';
    pub const COMPASS_CALIBRATION: char = '\u{E57C}';
    pub const COMPONENT_EXCHANGE: char = '\u{F1E7}';
    pub const COMPOST: char = '\u{E761}';
    pub const COMPRESS: char = '\u{E94D}';
    pub const COMPUTER: char = '\u{E31E}';
    pub const COMPUTER_ARROW_UP: char = '\u{F2F7}';
    pub const COMPUTER_CANCEL: char = '\u{F2F6}';
    pub const COMPUTER_SOUND: char = '\u{EEB4}';
    pub const CONCIERGE: char = '\u{F561}';
    pub const CONDITIONS: char = '\u{E0A0}';
    pub const CONFIRMATION_NUMBER: char = '\u{E638}';
    pub const CONGENITAL: char = '\u{E0A1}';
    pub const CONNECT_WITHOUT_CONTACT: char = '\u{F223}';
    pub const CONNECTED_TV: char = '\u{E998}';
    pub const CONNECTING_AIRPORTS: char = '\u{E7C9}';
    pub const CONSTRUCTION: char = '\u{EA3C}';
    pub const CONTACT_EMERGENCY: char = '\u{F8D1}';
    pub const CONTACT_MAIL: char = '\u{E0D0}';
    pub const CONTACT_PAGE: char = '\u{F22E}';
    pub const CONTACT_PHONE: char = '\u{F0C0}';
    pub const CONTACT_PHONE_FILLED: char = '\u{F0C0}';
    pub const CONTACT_SUPPORT: char = '\u{E94C}';
    pub const CONTACTLESS: char = '\u{EA71}';
    pub const CONTACTLESS_OFF: char = '\u{F858}';
    pub const CONTACTS: char = '\u{E0BA}';
    pub const CONTACTS_PRODUCT: char = '\u{E999}';
    pub const CONTENT_COPY: char = '\u{E14D}';
    pub const CONTENT_CUT: char = '\u{E14E}';
    pub const CONTENT_PASTE: char = '\u{E14F}';
    pub const CONTENT_PASTE_GO: char = '\u{EA8E}';
    pub const CONTENT_PASTE_OFF: char = '\u{E4F8}';
    pub const CONTENT_PASTE_SEARCH: char = '\u{EA9B}';
    pub const CONTEXTUAL_TOKEN: char = '\u{F486}';
    pub const CONTEXTUAL_TOKEN_ADD: char = '\u{F485}';
    pub const CONTRACT: char = '\u{F5A0}';
    pub const CONTRACT_DELETE: char = '\u{F5A2}';
    pub const CONTRACT_EDIT: char = '\u{F5A1}';
    pub const CONTRAST: char = '\u{EB37}';
    pub const CONTRAST_CIRCLE: char = '\u{F49F}';
    pub const CONTRAST_RTL_OFF: char = '\u{EC72}';
    pub const CONTRAST_SQUARE: char = '\u{F4A0}';
    pub const CONTROL_CAMERA: char = '\u{E074}';
    pub const CONTROL_POINT: char = '\u{E990}';
    pub const CONTROL_POINT_DUPLICATE: char = '\u{E3BB}';
    pub const CONTROLLER_GEN: char = '\u{E83D}';
    pub const CONVERSATION: char = '\u{EF2F}';
    pub const CONVERSION_PATH: char = '\u{F0C1}';
    pub const CONVERSION_PATH_OFF: char = '\u{F7B4}';
    pub const CONVERT_TO_TEXT: char = '\u{F41F}';
    pub const CONVEYOR_BELT: char = '\u{F867}';
    pub const COOKIE: char = '\u{EAAC}';
    pub const COOKIE_OFF: char = '\u{F79A}';
    pub const COOKING: char = '\u{E2B6}';
    pub const COOL_TO_DRY: char = '\u{E276}';
    pub const COPY_ALL: char = '\u{E2EC}';
    pub const COPYRIGHT: char = '\u{E90C}';
    pub const CORONAVIRUS: char = '\u{F221}';
    pub const CORPORATE_FARE: char = '\u{F1D0}';
    pub const COTTAGE: char = '\u{E587}';
    pub const COUNTER_0: char = '\u{F785}';
    pub const COUNTER_1: char = '\u{F784}';
    pub const COUNTER_2: char = '\u{F783}';
    pub const COUNTER_3: char = '\u{F782}';
    pub const COUNTER_4: char = '\u{F781}';
    pub const COUNTER_5: char = '\u{F780}';
    pub const COUNTER_6: char = '\u{F77F}';
    pub const COUNTER_7: char = '\u{F77E}';
    pub const COUNTER_8: char = '\u{F77D}';
    pub const COUNTER_9: char = '\u{F77C}';
    pub const COUNTERTOPS: char = '\u{F1F7}';
    pub const CREATE: char = '\u{F097}';
    pub const CREATE_NEW_FOLDER: char = '\u{E2CC}';
    pub const CREDIT_CARD: char = '\u{E8A1}';
    pub const CREDIT_CARD_CLOCK: char = '\u{F438}';
    pub const CREDIT_CARD_GEAR: char = '\u{F52D}';
    pub const CREDIT_CARD_HEART: char = '\u{F52C}';
    pub const CREDIT_CARD_OFF: char = '\u{E4F4}';
    pub const CREDIT_SCORE: char = '\u{EFF1}';
    pub const CRIB: char = '\u{E588}';
    pub const CRISIS_ALERT: char = '\u{EBE9}';
    pub const CROP: char = '\u{E3BE}';
    pub const CROP_16_9: char = '\u{E3BC}';
    pub const CROP_21_9: char = '\u{FFF0A}';
    pub const CROP_2_3: char = '\u{FFF0B}';
    pub const CROP_3_2: char = '\u{E3BD}';
    pub const CROP_5_4: char = '\u{E3BF}';
    pub const CROP_7_5: char = '\u{E3C0}';
    pub const CROP_9_16: char = '\u{F549}';
    pub const CROP_DIN: char = '\u{E3C6}';
    pub const CROP_FREE: char = '\u{E3C2}';
    pub const CROP_LANDSCAPE: char = '\u{E3C3}';
    pub const CROP_ORIGINAL: char = '\u{E3F4}';
    pub const CROP_PORTRAIT: char = '\u{E3C5}';
    pub const CROP_ROTATE: char = '\u{E437}';
    pub const CROP_SQUARE: char = '\u{E3C6}';
    pub const CROSSWORD: char = '\u{F5E5}';
    pub const CROWDSOURCE: char = '\u{EB18}';
    pub const CROWN: char = '\u{ECB3}';
    pub const CRUELTY_FREE: char = '\u{E799}';
    pub const CSS: char = '\u{EB93}';
    pub const CSV: char = '\u{E6CF}';
    pub const CURRENCY_BITCOIN: char = '\u{EBC5}';
    pub const CURRENCY_EXCHANGE: char = '\u{EB70}';
    pub const CURRENCY_FRANC: char = '\u{EAFA}';
    pub const CURRENCY_LIRA: char = '\u{EAEF}';
    pub const CURRENCY_POUND: char = '\u{EAF1}';
    pub const CURRENCY_RUBLE: char = '\u{EAEC}';
    pub const CURRENCY_RUPEE: char = '\u{EAF7}';
    pub const CURRENCY_RUPEE_CIRCLE: char = '\u{F460}';
    pub const CURRENCY_YEN: char = '\u{EAFB}';
    pub const CURRENCY_YUAN: char = '\u{EAF9}';
    pub const CURTAINS: char = '\u{EC1E}';
    pub const CURTAINS_CLOSED: char = '\u{EC1D}';
    pub const CUSTOM_TYPOGRAPHY: char = '\u{E732}';
    pub const CUT: char = '\u{F08B}';
    pub const CYCLE: char = '\u{F854}';
    pub const CYCLONE: char = '\u{EBD5}';
    pub const DANGEROUS: char = '\u{E99A}';
    pub const DARK_MODE: char = '\u{E51C}';
    pub const DASHBOARD: char = '\u{E871}';
    pub const DASHBOARD_2: char = '\u{F3EA}';
    pub const DASHBOARD_2_ADD: char = '\u{FFEE9}';
    pub const DASHBOARD_2_EDIT: char = '\u{FFFD7}';
    pub const DASHBOARD_2_GEAR: char = '\u{FFFD6}';
    pub const DASHBOARD_CUSTOMIZE: char = '\u{E99B}';
    pub const DATA_ALERT: char = '\u{F7F6}';
    pub const DATA_ARRAY: char = '\u{EAD1}';
    pub const DATA_CHECK: char = '\u{F7F2}';
    pub const DATA_EXPLORATION: char = '\u{E76F}';
    pub const DATA_INFO_ALERT: char = '\u{F7F5}';
    pub const DATA_LOSS_PREVENTION: char = '\u{E2DC}';
    pub const DATA_OBJECT: char = '\u{EAD3}';
    pub const DATA_SAVER_OFF: char = '\u{EFF2}';
    pub const DATA_SAVER_ON: char = '\u{EFF3}';
    pub const DATA_TABLE: char = '\u{E99C}';
    pub const DATA_THRESHOLDING: char = '\u{EB9F}';
    pub const DATA_USAGE: char = '\u{EFF2}';
    pub const DATABASE: char = '\u{F20E}';
    pub const DATABASE_OFF: char = '\u{F414}';
    pub const DATABASE_SEARCH: char = '\u{F38E}';
    pub const DATABASE_UPLOAD: char = '\u{F3DC}';
    pub const DATASET: char = '\u{F8EE}';
    pub const DATASET_LINKED: char = '\u{F8EF}';
    pub const DATE_RANGE: char = '\u{E916}';
    pub const DEBLUR: char = '\u{EB77}';
    pub const DECEASED: char = '\u{E0A5}';
    pub const DECIMAL_DECREASE: char = '\u{F82D}';
    pub const DECIMAL_INCREASE: char = '\u{F82C}';
    pub const DECK: char = '\u{EA42}';
    pub const DEHAZE: char = '\u{E3C7}';
    pub const DELETE: char = '\u{E92E}';
    pub const DELETE_FOREVER: char = '\u{E92B}';
    pub const DELETE_HISTORY: char = '\u{F518}';
    pub const DELETE_OUTLINE: char = '\u{E92E}';
    pub const DELETE_SWEEP: char = '\u{E16C}';
    pub const DELIVERY_DINING: char = '\u{EB28}';
    pub const DELIVERY_TRUCK_BOLT: char = '\u{F3A2}';
    pub const DELIVERY_TRUCK_SPEED: char = '\u{F3A1}';
    pub const DEMOGRAPHY: char = '\u{E489}';
    pub const DENSITY_LARGE: char = '\u{EBA9}';
    pub const DENSITY_MEDIUM: char = '\u{EB9E}';
    pub const DENSITY_SMALL: char = '\u{EBA8}';
    pub const DENTISTRY: char = '\u{E0A6}';
    pub const DEPARTURE_BOARD: char = '\u{E576}';
    pub const DEPLOYED_CODE: char = '\u{F720}';
    pub const DEPLOYED_CODE_ACCOUNT: char = '\u{F51B}';
    pub const DEPLOYED_CODE_ALERT: char = '\u{F5F2}';
    pub const DEPLOYED_CODE_HISTORY: char = '\u{F5F3}';
    pub const DEPLOYED_CODE_UPDATE: char = '\u{F5F4}';
    pub const DERMATOLOGY: char = '\u{E0A7}';
    pub const DESCRIPTION: char = '\u{E873}';
    pub const DESELECT: char = '\u{EBB6}';
    pub const DESIGN_SERVICES: char = '\u{F10A}';
    pub const DESK: char = '\u{F8F4}';
    pub const DESKPHONE: char = '\u{F7FA}';
    pub const DESKTOP_ACCESS_DISABLED: char = '\u{E99D}';
    pub const DESKTOP_CLOUD: char = '\u{F3DB}';
    pub const DESKTOP_CLOUD_STACK: char = '\u{F3BE}';
    pub const DESKTOP_LANDSCAPE: char = '\u{F45E}';
    pub const DESKTOP_LANDSCAPE_ADD: char = '\u{F439}';
    pub const DESKTOP_MAC: char = '\u{E30B}';
    pub const DESKTOP_PORTRAIT: char = '\u{F45D}';
    pub const DESKTOP_WINDOWS: char = '\u{E30C}';
    pub const DESTRUCTION: char = '\u{F585}';
    pub const DETAILS: char = '\u{E3C8}';
    pub const DETECTION_AND_ZONE: char = '\u{E29F}';
    pub const DETECTION_AND_ZONE_OFF: char = '\u{EEBF}';
    pub const DETECTOR: char = '\u{E282}';
    pub const DETECTOR_ALARM: char = '\u{E1F7}';
    pub const DETECTOR_BATTERY: char = '\u{E204}';
    pub const DETECTOR_CO: char = '\u{E2AF}';
    pub const DETECTOR_OFFLINE: char = '\u{E223}';
    pub const DETECTOR_SMOKE: char = '\u{E285}';
    pub const DETECTOR_STATUS: char = '\u{E1E8}';
    pub const DEVELOPER_BOARD: char = '\u{E30D}';
    pub const DEVELOPER_BOARD_OFF: char = '\u{E4FF}';
    pub const DEVELOPER_GUIDE: char = '\u{E99E}';
    pub const DEVELOPER_MODE: char = '\u{F2E2}';
    pub const DEVELOPER_MODE_TV: char = '\u{E874}';
    pub const DEVICE_BAND: char = '\u{F2F5}';
    pub const DEVICE_HUB: char = '\u{E335}';
    pub const DEVICE_RESET: char = '\u{E8B3}';
    pub const DEVICE_SWOOSH_STAR: char = '\u{FFEB8}';
    pub const DEVICE_THERMOSTAT: char = '\u{E1FF}';
    pub const DEVICE_UNKNOWN: char = '\u{F2E1}';
    pub const DEVICES: char = '\u{E326}';
    pub const DEVICES_FOLD: char = '\u{EBDE}';
    pub const DEVICES_FOLD_2: char = '\u{F406}';
    pub const DEVICES_OFF: char = '\u{F7A5}';
    pub const DEVICES_OTHER: char = '\u{E337}';
    pub const DEVICES_WEARABLES: char = '\u{F6AB}';
    pub const DEW_POINT: char = '\u{F879}';
    pub const DIAGNOSIS: char = '\u{E0A8}';
    pub const DIAGONAL_LINE: char = '\u{F41E}';
    pub const DIALER_SIP: char = '\u{E0BB}';
    pub const DIALOGS: char = '\u{E99F}';
    pub const DIALPAD: char = '\u{E0BC}';
    pub const DIAMOND: char = '\u{EAD5}';
    pub const DIAMOND_SHINE: char = '\u{F2B2}';
    pub const DICTIONARY: char = '\u{F539}';
    pub const DIFFERENCE: char = '\u{EB7D}';
    pub const DIGITAL_OUT_OF_HOME: char = '\u{F1DE}';
    pub const DIGITAL_WELLBEING: char = '\u{EF86}';
    pub const DINE_HEART: char = '\u{F29C}';
    pub const DINE_IN: char = '\u{F295}';
    pub const DINE_LAMP: char = '\u{F29B}';
    pub const DINING: char = '\u{EFF4}';
    pub const DINNER_DINING: char = '\u{EA57}';
    pub const DIRECTIONS: char = '\u{E52E}';
    pub const DIRECTIONS_ALT: char = '\u{F880}';
    pub const DIRECTIONS_ALT_OFF: char = '\u{F881}';
    pub const DIRECTIONS_BIKE: char = '\u{E52F}';
    pub const DIRECTIONS_BOAT: char = '\u{EFF5}';
    pub const DIRECTIONS_BOAT_FILLED: char = '\u{EFF5}';
    pub const DIRECTIONS_BUS: char = '\u{EFF6}';
    pub const DIRECTIONS_BUS_FILLED: char = '\u{EFF6}';
    pub const DIRECTIONS_CAR: char = '\u{EFF7}';
    pub const DIRECTIONS_CAR_FILLED: char = '\u{EFF7}';
    pub const DIRECTIONS_OFF: char = '\u{F10F}';
    pub const DIRECTIONS_RAILWAY: char = '\u{EFF8}';
    pub const DIRECTIONS_RAILWAY_2: char = '\u{F462}';
    pub const DIRECTIONS_RAILWAY_FILLED: char = '\u{EFF8}';
    pub const DIRECTIONS_RUN: char = '\u{E566}';
    pub const DIRECTIONS_SUBWAY: char = '\u{EFFA}';
    pub const DIRECTIONS_SUBWAY_FILLED: char = '\u{EFFA}';
    pub const DIRECTIONS_TRANSIT: char = '\u{EFFA}';
    pub const DIRECTIONS_TRANSIT_FILLED: char = '\u{EFFA}';
    pub const DIRECTIONS_WALK: char = '\u{E536}';
    pub const DIRECTORY_SYNC: char = '\u{E394}';
    pub const DIRTY_LENS: char = '\u{EF4B}';
    pub const DISABLED_BY_DEFAULT: char = '\u{F230}';
    pub const DISABLED_VISIBLE: char = '\u{E76E}';
    pub const DISC_FULL: char = '\u{E610}';
    pub const DISCOVER_TUNE: char = '\u{E018}';
    pub const DISHWASHER: char = '\u{E9A0}';
    pub const DISHWASHER_GEN: char = '\u{E832}';
    pub const DISPLAY_ADD: char = '\u{FFED2}';
    pub const DISPLAY_EXTERNAL_INPUT: char = '\u{F7E7}';
    pub const DISPLAY_SETTINGS: char = '\u{EB97}';
    pub const DISTANCE: char = '\u{F6EA}';
    pub const DIVERSITY_1: char = '\u{F8D7}';
    pub const DIVERSITY_2: char = '\u{F8D8}';
    pub const DIVERSITY_3: char = '\u{F8D9}';
    pub const DIVERSITY_4: char = '\u{F857}';
    pub const DNS: char = '\u{E875}';
    pub const DO_DISTURB: char = '\u{F08C}';
    pub const DO_DISTURB_ALT: char = '\u{F08D}';
    pub const DO_DISTURB_OFF: char = '\u{F08E}';
    pub const DO_DISTURB_ON: char = '\u{F08F}';
    pub const DO_NOT_DISTURB: char = '\u{F08D}';
    pub const DO_NOT_DISTURB_ALT: char = '\u{F08C}';
    pub const DO_NOT_DISTURB_OFF: char = '\u{F08E}';
    pub const DO_NOT_DISTURB_ON: char = '\u{F08F}';
    pub const DO_NOT_DISTURB_ON_TOTAL_SILENCE: char = '\u{EFFB}';
    pub const DO_NOT_STEP: char = '\u{F19F}';
    pub const DO_NOT_TOUCH: char = '\u{F1B0}';
    pub const DOCK: char = '\u{F2E0}';
    pub const DOCK_TO_BOTTOM: char = '\u{F7E6}';
    pub const DOCK_TO_LEFT: char = '\u{F7E5}';
    pub const DOCK_TO_RIGHT: char = '\u{F7E4}';
    pub const DOCS: char = '\u{EA7D}';
    pub const DOCS_ADD_ON: char = '\u{F0C2}';
    pub const DOCS_APPS_SCRIPT: char = '\u{F0C3}';
    pub const DOCUMENT_SCANNER: char = '\u{E5FA}';
    pub const DOCUMENT_SEARCH: char = '\u{F385}';
    pub const DOMAIN: char = '\u{E7EE}';
    pub const DOMAIN_ADD: char = '\u{EB62}';
    pub const DOMAIN_DISABLED: char = '\u{E0EF}';
    pub const DOMAIN_DISABLED_CHECK: char = '\u{FFEC6}';
    pub const DOMAIN_VERIFICATION: char = '\u{EF4C}';
    pub const DOMAIN_VERIFICATION_OFF: char = '\u{F7B0}';
    pub const DOMINO_MASK: char = '\u{F5E4}';
    pub const DONE: char = '\u{E876}';
    pub const DONE_ALL: char = '\u{E877}';
    pub const DONE_OUTLINE: char = '\u{E92F}';
    pub const DONUT_LARGE: char = '\u{E917}';
    pub const DONUT_SMALL: char = '\u{E918}';
    pub const DOOR_BACK: char = '\u{EFFC}';
    pub const DOOR_FRONT: char = '\u{EFFD}';
    pub const DOOR_OPEN: char = '\u{E77C}';
    pub const DOOR_SENSOR: char = '\u{E28A}';
    pub const DOOR_SLIDING: char = '\u{EFFE}';
    pub const DOORBELL: char = '\u{EFFF}';
    pub const DOORBELL_3P: char = '\u{E1E7}';
    pub const DOORBELL_CHIME: char = '\u{E1F3}';
    pub const DOUBLE_ARROW: char = '\u{EA50}';
    pub const DOWNHILL_SKIING: char = '\u{E509}';
    pub const DOWNLOAD: char = '\u{F090}';
    pub const DOWNLOAD_2: char = '\u{F523}';
    pub const DOWNLOAD_DONE: char = '\u{F091}';
    pub const DOWNLOAD_FOR_OFFLINE: char = '\u{F000}';
    pub const DOWNLOADING: char = '\u{F001}';
    pub const DRAFT: char = '\u{E66D}';
    pub const DRAFT_ORDERS: char = '\u{E7B3}';
    pub const DRAFTS: char = '\u{E151}';
    pub const DRAG_CLICK: char = '\u{F71F}';
    pub const DRAG_HANDLE: char = '\u{E25D}';
    pub const DRAG_INDICATOR: char = '\u{E945}';
    pub const DRAG_PAN: char = '\u{F71E}';
    pub const DRAW: char = '\u{E746}';
    pub const DRAW_ABSTRACT: char = '\u{F7F8}';
    pub const DRAW_COLLAGE: char = '\u{F7F7}';
    pub const DRAWING_RECOGNITION: char = '\u{EB00}';
    pub const DRESSER: char = '\u{E210}';
    pub const DRIVE_ETA: char = '\u{EFF7}';
    pub const DRIVE_EXPORT: char = '\u{F41D}';
    pub const DRIVE_FILE_MOVE: char = '\u{E9A1}';
    pub const DRIVE_FILE_MOVE_OUTLINE: char = '\u{E9A1}';
    pub const DRIVE_FILE_MOVE_RTL: char = '\u{E9A1}';
    pub const DRIVE_FILE_RENAME: char = '\u{E676}';
    pub const DRIVE_FILE_RENAME_OUTLINE: char = '\u{E9A2}';
    pub const DRIVE_FOLDER_UPLOAD: char = '\u{E9A3}';
    pub const DRONE: char = '\u{F25A}';
    pub const DRONE_2: char = '\u{F259}';
    pub const DROPDOWN: char = '\u{E9A4}';
    pub const DROPDOWN_MENU: char = '\u{FFEF0}';
    pub const DROPPER_EYE: char = '\u{F351}';
    pub const DRY: char = '\u{F1B3}';
    pub const DRY_CLEANING: char = '\u{EA58}';
    pub const DUAL_SCREEN: char = '\u{F6CF}';
    pub const DUO: char = '\u{E9A5}';
    pub const DVR: char = '\u{E1B2}';
    pub const DYNAMIC_FEED: char = '\u{EA14}';
    pub const DYNAMIC_FORM: char = '\u{F1BF}';
    pub const E911_AVATAR: char = '\u{F11A}';
    pub const E911_EMERGENCY: char = '\u{F119}';
    pub const E_MOBILEDATA: char = '\u{F002}';
    pub const E_MOBILEDATA_BADGE: char = '\u{F7E3}';
    pub const EAR_SOUND: char = '\u{F356}';
    pub const EARBUD_CASE: char = '\u{F327}';
    pub const EARBUD_LEFT: char = '\u{F326}';
    pub const EARBUD_RIGHT: char = '\u{F325}';
    pub const EARBUDS: char = '\u{F003}';
    pub const EARBUDS_2: char = '\u{F324}';
    pub const EARBUDS_BATTERY: char = '\u{F004}';
    pub const EARLY_ON: char = '\u{E2BA}';
    pub const EARTHQUAKE: char = '\u{F64F}';
    pub const EAST: char = '\u{F1DF}';
    pub const ECG: char = '\u{F80F}';
    pub const ECG_HEART: char = '\u{F6E9}';
    pub const ECO: char = '\u{EA35}';
    pub const EDA: char = '\u{F6E8}';
    pub const EDGESENSOR_HIGH: char = '\u{F2EF}';
    pub const EDGESENSOR_LOW: char = '\u{F2EE}';
    pub const EDIT: char = '\u{F097}';
    pub const EDIT_ARROW_DOWN: char = '\u{F380}';
    pub const EDIT_ARROW_UP: char = '\u{F37F}';
    pub const EDIT_ATTRIBUTES: char = '\u{E578}';
    pub const EDIT_AUDIO: char = '\u{F42D}';
    pub const EDIT_CALENDAR: char = '\u{E742}';
    pub const EDIT_DOCUMENT: char = '\u{F88C}';
    pub const EDIT_LOCATION: char = '\u{E568}';
    pub const EDIT_LOCATION_ALT: char = '\u{E1C5}';
    pub const EDIT_NOTE: char = '\u{E745}';
    pub const EDIT_NOTIFICATIONS: char = '\u{E525}';
    pub const EDIT_OFF: char = '\u{E950}';
    pub const EDIT_ROAD: char = '\u{EF4D}';
    pub const EDIT_SQUARE: char = '\u{F88D}';
    pub const EDITOR_CHOICE: char = '\u{F528}';
    pub const EGG: char = '\u{EACC}';
    pub const EGG_ALT: char = '\u{EAC8}';
    pub const EJECT: char = '\u{E8FB}';
    pub const ELDERLY: char = '\u{F21A}';
    pub const ELDERLY_WOMAN: char = '\u{EB69}';
    pub const ELECTRIC_BIKE: char = '\u{EB1B}';
    pub const ELECTRIC_BOLT: char = '\u{EC1C}';
    pub const ELECTRIC_CAR: char = '\u{EB1C}';
    pub const ELECTRIC_METER: char = '\u{EC1B}';
    pub const ELECTRIC_MOPED: char = '\u{EB1D}';
    pub const ELECTRIC_RICKSHAW: char = '\u{EB1E}';
    pub const ELECTRIC_SCOOTER: char = '\u{EB1F}';
    pub const ELECTRICAL_SERVICES: char = '\u{F102}';
    pub const ELEVATION: char = '\u{F6E7}';
    pub const ELEVATOR: char = '\u{F1A0}';
    pub const EMAIL: char = '\u{E159}';
    pub const EMERGENCY: char = '\u{E1EB}';
    pub const EMERGENCY_HEAT: char = '\u{F15D}';
    pub const EMERGENCY_HEAT_2: char = '\u{F4E5}';
    pub const EMERGENCY_HOME: char = '\u{E82A}';
    pub const EMERGENCY_RECORDING: char = '\u{EBF4}';
    pub const EMERGENCY_SHARE: char = '\u{EBF6}';
    pub const EMERGENCY_SHARE_OFF: char = '\u{F59E}';
    pub const EMOJI_EMOTIONS: char = '\u{EA22}';
    pub const EMOJI_EVENTS: char = '\u{EA23}';
    pub const EMOJI_FLAGS: char = '\u{F0C6}';
    pub const EMOJI_FOOD_BEVERAGE: char = '\u{EA1B}';
    pub const EMOJI_LANGUAGE: char = '\u{F4CD}';
    pub const EMOJI_NATURE: char = '\u{EA1C}';
    pub const EMOJI_OBJECTS: char = '\u{EA24}';
    pub const EMOJI_PEOPLE: char = '\u{EA1D}';
    pub const EMOJI_SYMBOLS: char = '\u{EA1E}';
    pub const EMOJI_TRANSPORTATION: char = '\u{EA1F}';
    pub const EMOTICON: char = '\u{E5F3}';
    pub const EMPTY_DASHBOARD: char = '\u{F844}';
    pub const ENABLE: char = '\u{F188}';
    pub const ENCRYPTED: char = '\u{E593}';
    pub const ENCRYPTED_ADD: char = '\u{F429}';
    pub const ENCRYPTED_ADD_CIRCLE: char = '\u{F42A}';
    pub const ENCRYPTED_MINUS_CIRCLE: char = '\u{F428}';
    pub const ENCRYPTED_OFF: char = '\u{F427}';
    pub const ENDOCRINOLOGY: char = '\u{E0A9}';
    pub const ENERGY: char = '\u{E9A6}';
    pub const ENERGY_PROGRAM_SAVING: char = '\u{F15F}';
    pub const ENERGY_PROGRAM_TIME_USED: char = '\u{F161}';
    pub const ENERGY_SAVINGS_LEAF: char = '\u{EC1A}';
    pub const ENGINEERING: char = '\u{EA3D}';
    pub const ENHANCED_ENCRYPTION: char = '\u{E63F}';
    pub const ENT: char = '\u{E0AA}';
    pub const ENTERPRISE: char = '\u{E70E}';
    pub const ENTERPRISE_OFF: char = '\u{EB4D}';
    pub const EQUAL: char = '\u{F77B}';
    pub const EQUALIZER: char = '\u{E01D}';
    pub const ERASER_SIZE_1: char = '\u{F3FC}';
    pub const ERASER_SIZE_2: char = '\u{F3FB}';
    pub const ERASER_SIZE_3: char = '\u{F3FA}';
    pub const ERASER_SIZE_4: char = '\u{F3F9}';
    pub const ERASER_SIZE_5: char = '\u{F3F8}';
    pub const ERROR: char = '\u{F8B6}';
    pub const ERROR_CIRCLE_ROUNDED: char = '\u{F8B6}';
    pub const ERROR_MED: char = '\u{E49B}';
    pub const ERROR_OUTLINE: char = '\u{F8B6}';
    pub const ESCALATOR: char = '\u{F1A1}';
    pub const ESCALATOR_WARNING: char = '\u{F1AC}';
    pub const EURO: char = '\u{EA15}';
    pub const EURO_SYMBOL: char = '\u{E926}';
    pub const EV_CHARGER: char = '\u{E56D}';
    pub const EV_MOBILEDATA_BADGE: char = '\u{F7E2}';
    pub const EV_SHADOW: char = '\u{EF8F}';
    pub const EV_SHADOW_ADD: char = '\u{F580}';
    pub const EV_SHADOW_MINUS: char = '\u{F57F}';
    pub const EV_STATION: char = '\u{E56D}';
    pub const EVENT: char = '\u{E878}';
    pub const EVENT_AVAILABLE: char = '\u{E614}';
    pub const EVENT_BUSY: char = '\u{E615}';
    pub const EVENT_LIST: char = '\u{F683}';
    pub const EVENT_NOTE: char = '\u{E616}';
    pub const EVENT_REPEAT: char = '\u{EB7B}';
    pub const EVENT_SEAT: char = '\u{E903}';
    pub const EVENT_UPCOMING: char = '\u{F238}';
    pub const EXCLAMATION: char = '\u{F22F}';
    pub const EXERCISE: char = '\u{F6E6}';
    pub const EXIT_TO_APP: char = '\u{E879}';
    pub const EXPAND: char = '\u{E94F}';
    pub const EXPAND_ALL: char = '\u{E946}';
    pub const EXPAND_CIRCLE_DOWN: char = '\u{E7CD}';
    pub const EXPAND_CIRCLE_RIGHT: char = '\u{F591}';
    pub const EXPAND_CIRCLE_UP: char = '\u{F5D2}';
    pub const EXPAND_CONTENT: char = '\u{F830}';
    pub const EXPAND_LESS: char = '\u{E5CE}';
    pub const EXPAND_MORE: char = '\u{E5CF}';
    pub const EXPANSION_PANELS: char = '\u{EF90}';
    pub const EXPENSION_PANELS: char = '\u{EF90}';
    pub const EXPERIMENT: char = '\u{E686}';
    pub const EXPLICIT: char = '\u{E01E}';
    pub const EXPLORE: char = '\u{E87A}';
    pub const EXPLORE_NEARBY: char = '\u{E538}';
    pub const EXPLORE_OFF: char = '\u{E9A8}';
    pub const EXPLOSION: char = '\u{F685}';
    pub const EXPORT_NOTES: char = '\u{E0AC}';
    pub const EXPOSURE: char = '\u{E3F6}';
    pub const EXPOSURE_NEG_1: char = '\u{E3CB}';
    pub const EXPOSURE_NEG_2: char = '\u{E3CC}';
    pub const EXPOSURE_PLUS_1: char = '\u{E800}';
    pub const EXPOSURE_PLUS_2: char = '\u{E3CE}';
    pub const EXPOSURE_ZERO: char = '\u{E3CF}';
    pub const EXTENSION: char = '\u{E87B}';
    pub const EXTENSION_OFF: char = '\u{E4F5}';
    pub const EYE_TRACKING: char = '\u{F4C9}';
    pub const EYEBROW: char = '\u{EEB3}';
    pub const EYEGLASSES: char = '\u{F6EE}';
    pub const EYEGLASSES_2: char = '\u{F2C7}';
    pub const EYEGLASSES_2_SOUND: char = '\u{F265}';
    pub const EYEGLASSES_3: char = '\u{FFEF1}';
    pub const FACE: char = '\u{F008}';
    pub const FACE_2: char = '\u{F8DA}';
    pub const FACE_3: char = '\u{F8DB}';
    pub const FACE_4: char = '\u{F8DC}';
    pub const FACE_5: char = '\u{F8DD}';
    pub const FACE_6: char = '\u{F8DE}';
    pub const FACE_DOWN: char = '\u{F402}';
    pub const FACE_LEFT: char = '\u{F401}';
    pub const FACE_NOD: char = '\u{F400}';
    pub const FACE_RETOUCHING_NATURAL: char = '\u{EF4E}';
    pub const FACE_RETOUCHING_OFF: char = '\u{F007}';
    pub const FACE_RIGHT: char = '\u{F3FF}';
    pub const FACE_SHAKE: char = '\u{F3FE}';
    pub const FACE_UNLOCK: char = '\u{F008}';
    pub const FACE_UP: char = '\u{F3FD}';
    pub const FACT_CHECK: char = '\u{F0C5}';
    pub const FACTORY: char = '\u{EBBC}';
    pub const FALLING: char = '\u{F60D}';
    pub const FAMILIAR_FACE_AND_ZONE: char = '\u{E21C}';
    pub const FAMILY_GROUP: char = '\u{EEF2}';
    pub const FAMILY_HISTORY: char = '\u{E0AD}';
    pub const FAMILY_HOME: char = '\u{EB26}';
    pub const FAMILY_LINK: char = '\u{EB19}';
    pub const FAMILY_RESTROOM: char = '\u{F1A2}';
    pub const FAMILY_STAR: char = '\u{F527}';
    pub const FAN_FOCUS: char = '\u{F334}';
    pub const FAN_INDIRECT: char = '\u{F333}';
    pub const FARSIGHT_DIGITAL: char = '\u{F559}';
    pub const FAST_FORWARD: char = '\u{E01F}';
    pub const FAST_REWIND: char = '\u{E020}';
    pub const FASTFOOD: char = '\u{E57A}';
    pub const FAUCET: char = '\u{E278}';
    pub const FAVORITE: char = '\u{E87E}';
    pub const FAVORITE_BORDER: char = '\u{E87E}';
    pub const FAX: char = '\u{EAD8}';
    pub const FEATURE_SEARCH: char = '\u{E9A9}';
    pub const FEATURED_PLAY_LIST: char = '\u{E06D}';
    pub const FEATURED_SEASONAL_AND_GIFTS: char = '\u{EF91}';
    pub const FEATURED_VIDEO: char = '\u{E06E}';
    pub const FEED: char = '\u{F009}';
    pub const FEEDBACK: char = '\u{E87F}';
    pub const FEMALE: char = '\u{E590}';
    pub const FEMUR: char = '\u{F891}';
    pub const FEMUR_ALT: char = '\u{F892}';
    pub const FENCE: char = '\u{F1F6}';
    pub const FERTILE: char = '\u{F6E5}';
    pub const FESTIVAL: char = '\u{EA68}';
    pub const FIBER_DVR: char = '\u{E05D}';
    pub const FIBER_MANUAL_RECORD: char = '\u{E061}';
    pub const FIBER_NEW: char = '\u{E05E}';
    pub const FIBER_PIN: char = '\u{E06A}';
    pub const FIBER_SMART_RECORD: char = '\u{E062}';
    pub const FILE_COPY: char = '\u{E173}';
    pub const FILE_COPY_OFF: char = '\u{F4D8}';
    pub const FILE_DOWNLOAD: char = '\u{F090}';
    pub const FILE_DOWNLOAD_DONE: char = '\u{F091}';
    pub const FILE_DOWNLOAD_OFF: char = '\u{E4FE}';
    pub const FILE_EXPORT: char = '\u{F3B2}';
    pub const FILE_JSON: char = '\u{F3BB}';
    pub const FILE_MAP: char = '\u{E2C5}';
    pub const FILE_MAP_STACK: char = '\u{F3E2}';
    pub const FILE_OPEN: char = '\u{EAF3}';
    pub const FILE_PNG: char = '\u{F3BC}';
    pub const FILE_PRESENT: char = '\u{EA0E}';
    pub const FILE_SAVE: char = '\u{F17F}';
    pub const FILE_SAVE_OFF: char = '\u{E505}';
    pub const FILE_UPLOAD: char = '\u{F09B}';
    pub const FILE_UPLOAD_OFF: char = '\u{F886}';
    pub const FILES: char = '\u{EA85}';
    pub const FILTER: char = '\u{E3D3}';
    pub const FILTER_1: char = '\u{E3D0}';
    pub const FILTER_2: char = '\u{E3D1}';
    pub const FILTER_3: char = '\u{E3D2}';
    pub const FILTER_4: char = '\u{E3D4}';
    pub const FILTER_5: char = '\u{E3D5}';
    pub const FILTER_6: char = '\u{E3D6}';
    pub const FILTER_7: char = '\u{E3D7}';
    pub const FILTER_8: char = '\u{E3D8}';
    pub const FILTER_9: char = '\u{E3D9}';
    pub const FILTER_9_PLUS: char = '\u{E3DA}';
    pub const FILTER_ALT: char = '\u{EF4F}';
    pub const FILTER_ALT_OFF: char = '\u{EB32}';
    pub const FILTER_ARROW_RIGHT: char = '\u{F3D1}';
    pub const FILTER_B_AND_W: char = '\u{E3DB}';
    pub const FILTER_CENTER_FOCUS: char = '\u{E3DC}';
    pub const FILTER_DRAMA: char = '\u{E3DD}';
    pub const FILTER_FRAMES: char = '\u{E3DE}';
    pub const FILTER_HDR: char = '\u{E3DF}';
    pub const FILTER_LIST: char = '\u{E152}';
    pub const FILTER_LIST_ALT: char = '\u{E94E}';
    pub const FILTER_LIST_OFF: char = '\u{EB57}';
    pub const FILTER_NONE: char = '\u{E3E0}';
    pub const FILTER_RETROLUX: char = '\u{E3E1}';
    pub const FILTER_TILT_SHIFT: char = '\u{E3E2}';
    pub const FILTER_VINTAGE: char = '\u{E3E3}';
    pub const FINANCE: char = '\u{E6BF}';
    pub const FINANCE_CHIP: char = '\u{F84E}';
    pub const FINANCE_MODE: char = '\u{EF92}';
    pub const FIND_IN_PAGE: char = '\u{E880}';
    pub const FIND_REPLACE: char = '\u{E881}';
    pub const FINGERPRINT: char = '\u{E90D}';
    pub const FINGERPRINT_OFF: char = '\u{F49D}';
    pub const FIRE_CHECK: char = '\u{FFFA8}';
    pub const FIRE_EXTINGUISHER: char = '\u{F1D8}';
    pub const FIRE_HYDRANT: char = '\u{F1A3}';
    pub const FIRE_TRUCK: char = '\u{F8F2}';
    pub const FIREPLACE: char = '\u{EA43}';
    pub const FIRST_PAGE: char = '\u{E5DC}';
    pub const FIT_PAGE: char = '\u{F77A}';
    pub const FIT_PAGE_HEIGHT: char = '\u{F397}';
    pub const FIT_PAGE_WIDTH: char = '\u{F396}';
    pub const FIT_SCREEN: char = '\u{EA10}';
    pub const FIT_WIDTH: char = '\u{F779}';
    pub const FITNESS_CENTER: char = '\u{EB43}';
    pub const FITNESS_TRACKER: char = '\u{F463}';
    pub const FITNESS_TRACKERS: char = '\u{EEF1}';
    pub const FLAG: char = '\u{F0C6}';
    pub const FLAG_2: char = '\u{F40F}';
    pub const FLAG_CHECK: char = '\u{F3D8}';
    pub const FLAG_CIRCLE: char = '\u{EAF8}';
    pub const FLAG_FILLED: char = '\u{F0C6}';
    pub const FLAKY: char = '\u{EF50}';
    pub const FLARE: char = '\u{E3E4}';
    pub const FLASH_AUTO: char = '\u{E3E5}';
    pub const FLASH_OFF: char = '\u{E3E6}';
    pub const FLASH_ON: char = '\u{E3E7}';
    pub const FLASHLIGHT_OFF: char = '\u{F00A}';
    pub const FLASHLIGHT_ON: char = '\u{F00B}';
    pub const FLATWARE: char = '\u{F00C}';
    pub const FLEX_DIRECTION: char = '\u{F778}';
    pub const FLEX_NO_WRAP: char = '\u{F777}';
    pub const FLEX_WRAP: char = '\u{F776}';
    pub const FLIGHT: char = '\u{E539}';
    pub const FLIGHT_CLASS: char = '\u{E7CB}';
    pub const FLIGHT_LAND: char = '\u{E904}';
    pub const FLIGHT_TAKEOFF: char = '\u{E905}';
    pub const FLIGHTS_AND_HOTELS: char = '\u{E9AB}';
    pub const FLIGHTSMODE: char = '\u{EF93}';
    pub const FLIP: char = '\u{E3E8}';
    pub const FLIP_CAMERA_ANDROID: char = '\u{EA37}';
    pub const FLIP_CAMERA_IOS: char = '\u{EA38}';
    pub const FLIP_TO_BACK: char = '\u{E882}';
    pub const FLIP_TO_FRONT: char = '\u{E883}';
    pub const FLOAT_LANDSCAPE_2: char = '\u{F45C}';
    pub const FLOAT_PORTRAIT_2: char = '\u{F45B}';
    pub const FLOOD: char = '\u{EBE6}';
    pub const FLOOR: char = '\u{F6E4}';
    pub const FLOOR_LAMP: char = '\u{E21E}';
    pub const FLOURESCENT: char = '\u{F07D}';
    pub const FLOWCHART: char = '\u{F38D}';
    pub const FLOWSHEET: char = '\u{E0AE}';
    pub const FLUID: char = '\u{E483}';
    pub const FLUID_BALANCE: char = '\u{F80D}';
    pub const FLUID_MED: char = '\u{F80C}';
    pub const FLUORESCENT: char = '\u{F07D}';
    pub const FLUTTER: char = '\u{F1DD}';
    pub const FLUTTER_DASH: char = '\u{E00B}';
    pub const FLYOVER: char = '\u{F478}';
    pub const FMD_BAD: char = '\u{F00E}';
    pub const FMD_GOOD: char = '\u{F1DB}';
    pub const FOGGY: char = '\u{E818}';
    pub const FOLDED_HANDS: char = '\u{F5ED}';
    pub const FOLDER: char = '\u{E2C7}';
    pub const FOLDER_CHECK: char = '\u{F3D7}';
    pub const FOLDER_CHECK_2: char = '\u{F3D6}';
    pub const FOLDER_CODE: char = '\u{F3C8}';
    pub const FOLDER_COPY: char = '\u{EBBD}';
    pub const FOLDER_DATA: char = '\u{F586}';
    pub const FOLDER_DELETE: char = '\u{EB34}';
    pub const FOLDER_EYE: char = '\u{F3D5}';
    pub const FOLDER_INFO: char = '\u{F395}';
    pub const FOLDER_LIMITED: char = '\u{F4E4}';
    pub const FOLDER_MANAGED: char = '\u{F775}';
    pub const FOLDER_MATCH: char = '\u{F3D4}';
    pub const FOLDER_OFF: char = '\u{EB83}';
    pub const FOLDER_OPEN: char = '\u{E2C8}';
    pub const FOLDER_SHARED: char = '\u{E2C9}';
    pub const FOLDER_SPECIAL: char = '\u{E617}';
    pub const FOLDER_SUPERVISED: char = '\u{F774}';
    pub const FOLDER_ZIP: char = '\u{EB2C}';
    pub const FOLLOW_THE_SIGNS: char = '\u{F222}';
    pub const FONT_DOWNLOAD: char = '\u{E167}';
    pub const FONT_DOWNLOAD_OFF: char = '\u{E4F9}';
    pub const FOOD_BANK: char = '\u{F1F2}';
    pub const FOOT_BONES: char = '\u{F893}';
    pub const FOOTPRINT: char = '\u{F87D}';
    pub const FOR_YOU: char = '\u{E9AC}';
    pub const FOREST: char = '\u{EA99}';
    pub const FORK_CHART: char = '\u{FFFA6}';
    pub const FORK_LEFT: char = '\u{EBA0}';
    pub const FORK_RIGHT: char = '\u{EBAC}';
    pub const FORK_SPOON: char = '\u{F3E4}';
    pub const FORKLIFT: char = '\u{F868}';
    pub const FORMAT_ALIGN_CENTER: char = '\u{E234}';
    pub const FORMAT_ALIGN_JUSTIFY: char = '\u{E235}';
    pub const FORMAT_ALIGN_LEFT: char = '\u{E236}';
    pub const FORMAT_ALIGN_RIGHT: char = '\u{E237}';
    pub const FORMAT_BOLD: char = '\u{E238}';
    pub const FORMAT_CLEAR: char = '\u{E239}';
    pub const FORMAT_COLOR_FILL: char = '\u{E23A}';
    pub const FORMAT_COLOR_RESET: char = '\u{E23B}';
    pub const FORMAT_COLOR_TEXT: char = '\u{E23C}';
    pub const FORMAT_H1: char = '\u{F85D}';
    pub const FORMAT_H2: char = '\u{F85E}';
    pub const FORMAT_H3: char = '\u{F85F}';
    pub const FORMAT_H4: char = '\u{F860}';
    pub const FORMAT_H5: char = '\u{F861}';
    pub const FORMAT_H6: char = '\u{F862}';
    pub const FORMAT_IMAGE_BACK: char = '\u{EEB0}';
    pub const FORMAT_IMAGE_BREAK_LEFT: char = '\u{EEAF}';
    pub const FORMAT_IMAGE_BREAK_RIGHT: char = '\u{EEAE}';
    pub const FORMAT_IMAGE_FRONT: char = '\u{EEAD}';
    pub const FORMAT_IMAGE_INLINE_LEFT: char = '\u{EEAC}';
    pub const FORMAT_IMAGE_INLINE_RIGHT: char = '\u{FFFFD}';
    pub const FORMAT_IMAGE_LEFT: char = '\u{F863}';
    pub const FORMAT_IMAGE_RIGHT: char = '\u{F864}';
    pub const FORMAT_INDENT_DECREASE: char = '\u{E23D}';
    pub const FORMAT_INDENT_INCREASE: char = '\u{E23E}';
    pub const FORMAT_INK_HIGHLIGHTER: char = '\u{F82B}';
    pub const FORMAT_ITALIC: char = '\u{E23F}';
    pub const FORMAT_LETTER_SPACING: char = '\u{F773}';
    pub const FORMAT_LETTER_SPACING_2: char = '\u{F618}';
    pub const FORMAT_LETTER_SPACING_STANDARD: char = '\u{F617}';
    pub const FORMAT_LETTER_SPACING_WIDE: char = '\u{F616}';
    pub const FORMAT_LETTER_SPACING_WIDER: char = '\u{F615}';
    pub const FORMAT_LINE_SPACING: char = '\u{E240}';
    pub const FORMAT_LIST_BULLETED: char = '\u{E241}';
    pub const FORMAT_LIST_BULLETED_ADD: char = '\u{F849}';
    pub const FORMAT_LIST_NUMBERED: char = '\u{E242}';
    pub const FORMAT_LIST_NUMBERED_RTL: char = '\u{E267}';
    pub const FORMAT_OVERLINE: char = '\u{EB65}';
    pub const FORMAT_PAINT: char = '\u{E243}';
    pub const FORMAT_PAINT_OFF: char = '\u{FFF97}';
    pub const FORMAT_PARAGRAPH: char = '\u{F865}';
    pub const FORMAT_QUOTE: char = '\u{E244}';
    pub const FORMAT_QUOTE_OFF: char = '\u{F413}';
    pub const FORMAT_SHAPES: char = '\u{E25E}';
    pub const FORMAT_SIZE: char = '\u{E245}';
    pub const FORMAT_STRIKETHROUGH: char = '\u{E246}';
    pub const FORMAT_TEXT_CLIP: char = '\u{F82A}';
    pub const FORMAT_TEXT_OVERFLOW: char = '\u{F829}';
    pub const FORMAT_TEXT_WRAP: char = '\u{F828}';
    pub const FORMAT_TEXTDIRECTION_L_TO_R: char = '\u{E247}';
    pub const FORMAT_TEXTDIRECTION_R_TO_L: char = '\u{E248}';
    pub const FORMAT_TEXTDIRECTION_VERTICAL: char = '\u{F4B8}';
    pub const FORMAT_UNDERLINED: char = '\u{E249}';
    pub const FORMAT_UNDERLINED_SQUIGGLE: char = '\u{F885}';
    pub const FORMS_ADD_ON: char = '\u{F0C7}';
    pub const FORMS_APPS_SCRIPT: char = '\u{F0C8}';
    pub const FORT: char = '\u{EAAD}';
    pub const FORUM: char = '\u{E8AF}';
    pub const FORWARD: char = '\u{F57A}';
    pub const FORWARD_10: char = '\u{E056}';
    pub const FORWARD_30: char = '\u{E057}';
    pub const FORWARD_5: char = '\u{E058}';
    pub const FORWARD_CIRCLE: char = '\u{F6F5}';
    pub const FORWARD_MEDIA: char = '\u{F6F4}';
    pub const FORWARD_TO_INBOX: char = '\u{F187}';
    pub const FOUNDATION: char = '\u{F200}';
    pub const FRAGRANCE: char = '\u{F345}';
    pub const FRAME_BUG: char = '\u{EEEF}';
    pub const FRAME_EXCLAMATION: char = '\u{EEEE}';
    pub const FRAME_INSPECT: char = '\u{F772}';
    pub const FRAME_PERSON: char = '\u{F8A6}';
    pub const FRAME_PERSON_MIC: char = '\u{F4D5}';
    pub const FRAME_PERSON_OFF: char = '\u{F7D1}';
    pub const FRAME_RELOAD: char = '\u{F771}';
    pub const FRAME_SOURCE: char = '\u{F770}';
    pub const FREE_BREAKFAST: char = '\u{EB44}';
    pub const FREE_CANCELLATION: char = '\u{E748}';
    pub const FRONT_HAND: char = '\u{E769}';
    pub const FRONT_LOADER: char = '\u{F869}';
    pub const FULL_COVERAGE: char = '\u{EB12}';
    pub const FULL_HD: char = '\u{F58B}';
    pub const FULL_STACKED_BAR_CHART: char = '\u{F212}';
    pub const FULLSCREEN: char = '\u{E5D0}';
    pub const FULLSCREEN_EXIT: char = '\u{E5D1}';
    pub const FULLSCREEN_PORTRAIT: char = '\u{F45A}';
    pub const FUNCTION: char = '\u{F866}';
    pub const FUNCTIONS: char = '\u{E24A}';
    pub const FUNICULAR: char = '\u{F477}';
    pub const G_MOBILEDATA: char = '\u{F010}';
    pub const G_MOBILEDATA_BADGE: char = '\u{F7E1}';
    pub const G_TRANSLATE: char = '\u{E927}';
    pub const GALLERY_THUMBNAIL: char = '\u{F86F}';
    pub const GAME_BUMPER_LEFT: char = '\u{EEE0}';
    pub const GAME_BUMPER_RIGHT: char = '\u{EEDF}';
    pub const GAME_BUTTON_L: char = '\u{EEDE}';
    pub const GAME_BUTTON_L1: char = '\u{EEDD}';
    pub const GAME_BUTTON_L2: char = '\u{EEDC}';
    pub const GAME_BUTTON_R: char = '\u{EEDB}';
    pub const GAME_BUTTON_R1: char = '\u{EEDA}';
    pub const GAME_BUTTON_R2: char = '\u{EED9}';
    pub const GAME_BUTTON_ZL: char = '\u{EED8}';
    pub const GAME_BUTTON_ZR: char = '\u{EED7}';
    pub const GAME_STICK_L3: char = '\u{EED6}';
    pub const GAME_STICK_LEFT: char = '\u{EED5}';
    pub const GAME_STICK_R3: char = '\u{EED4}';
    pub const GAME_STICK_RIGHT: char = '\u{EED3}';
    pub const GAME_TRIGGER_LEFT: char = '\u{EED2}';
    pub const GAME_TRIGGER_RIGHT: char = '\u{EED1}';
    pub const GAMEPAD: char = '\u{E30F}';
    pub const GAMEPAD_CIRCLE_DOWN: char = '\u{EED0}';
    pub const GAMEPAD_CIRCLE_LEFT: char = '\u{EECF}';
    pub const GAMEPAD_CIRCLE_RIGHT: char = '\u{EECE}';
    pub const GAMEPAD_CIRCLE_UP: char = '\u{EECD}';
    pub const GAMEPAD_DOWN: char = '\u{EECC}';
    pub const GAMEPAD_LEFT: char = '\u{EECB}';
    pub const GAMEPAD_RIGHT: char = '\u{EECA}';
    pub const GAMEPAD_UP: char = '\u{EEC9}';
    pub const GAMES: char = '\u{E30F}';
    pub const GARAGE: char = '\u{F011}';
    pub const GARAGE_CHECK: char = '\u{F28D}';
    pub const GARAGE_DOOR: char = '\u{E714}';
    pub const GARAGE_DOOR_OPEN: char = '\u{FFF77}';
    pub const GARAGE_HOME: char = '\u{E82D}';
    pub const GARAGE_MONEY: char = '\u{F28C}';
    pub const GARDEN_CART: char = '\u{F8A9}';
    pub const GAS_METER: char = '\u{EC19}';
    pub const GASTROENTEROLOGY: char = '\u{E0F1}';
    pub const GATE: char = '\u{E277}';
    pub const GAVEL: char = '\u{E90E}';
    pub const GENERAL_DEVICE: char = '\u{E6DE}';
    pub const GENERATING_TOKENS: char = '\u{E749}';
    pub const GENETICS: char = '\u{E0F3}';
    pub const GENRES: char = '\u{E6EE}';
    pub const GESTURE: char = '\u{E155}';
    pub const GESTURE_SELECT: char = '\u{F657}';
    pub const GET_APP: char = '\u{F090}';
    pub const GIF: char = '\u{E908}';
    pub const GIF_2: char = '\u{F40E}';
    pub const GIF_BOX: char = '\u{E7A3}';
    pub const GIRL: char = '\u{EB68}';
    pub const GITE: char = '\u{E58B}';
    pub const GLASS_CUP: char = '\u{F6E3}';
    pub const GLOBE: char = '\u{E64C}';
    pub const GLOBE_2_CANCEL: char = '\u{FFFB7}';
    pub const GLOBE_2_QUESTION: char = '\u{FFFB6}';
    pub const GLOBE_ASIA: char = '\u{F799}';
    pub const GLOBE_BOOK: char = '\u{F3C9}';
    pub const GLOBE_CLOCK: char = '\u{FFED1}';
    pub const GLOBE_LOCATION_PIN: char = '\u{F35D}';
    pub const GLOBE_UK: char = '\u{F798}';
    pub const GLUCOSE: char = '\u{E4A0}';
    pub const GLYPHS: char = '\u{F8A3}';
    pub const GO_TO_LINE: char = '\u{F71D}';
    pub const GOLF_COURSE: char = '\u{EB45}';
    pub const GONDOLA_LIFT: char = '\u{F476}';
    pub const GOOGLE_HOME_DEVICES: char = '\u{E715}';
    pub const GOOGLE_PLUS_RESHARE: char = '\u{F57A}';
    pub const GOOGLE_TV_REMOTE: char = '\u{F5DB}';
    pub const GOOGLE_WIFI: char = '\u{F579}';
    pub const GPP_BAD: char = '\u{F012}';
    pub const GPP_GOOD: char = '\u{F013}';
    pub const GPP_MAYBE: char = '\u{F014}';
    pub const GPS_FIXED: char = '\u{E55C}';
    pub const GPS_NOT_FIXED: char = '\u{E1B7}';
    pub const GPS_OFF: char = '\u{E1B6}';
    pub const GRADE: char = '\u{F09A}';
    pub const GRADIENT: char = '\u{E3E9}';
    pub const GRADING: char = '\u{EA4F}';
    pub const GRAIN: char = '\u{E3EA}';
    pub const GRAPH_1: char = '\u{F3A0}';
    pub const GRAPH_2: char = '\u{F39F}';
    pub const GRAPH_3: char = '\u{F39E}';
    pub const GRAPH_4: char = '\u{F39D}';
    pub const GRAPH_5: char = '\u{F39C}';
    pub const GRAPH_6: char = '\u{F39B}';
    pub const GRAPH_7: char = '\u{F346}';
    pub const GRAPH_8: char = '\u{FFFEC}';
    pub const GRAPHIC_EQ: char = '\u{E1B8}';
    pub const GRAPHIC_EQ_OFF: char = '\u{FFF98}';
    pub const GRASS: char = '\u{F205}';
    pub const GRID_3X3: char = '\u{F015}';
    pub const GRID_3X3_OFF: char = '\u{F67C}';
    pub const GRID_4X4: char = '\u{F016}';
    pub const GRID_GOLDENRATIO: char = '\u{F017}';
    pub const GRID_GUIDES: char = '\u{F76F}';
    pub const GRID_LAYOUT_SIDE: char = '\u{FFF8D}';
    pub const GRID_OFF: char = '\u{E3EB}';
    pub const GRID_ON: char = '\u{E3EC}';
    pub const GRID_VIEW: char = '\u{E9B0}';
    pub const GROCERY: char = '\u{EF97}';
    pub const GROUP: char = '\u{EA21}';
    pub const GROUP_ADD: char = '\u{E7F0}';
    pub const GROUP_OFF: char = '\u{E747}';
    pub const GROUP_REMOVE: char = '\u{E7AD}';
    pub const GROUP_SEARCH: char = '\u{F3CE}';
    pub const GROUP_WORK: char = '\u{E886}';
    pub const GROUPED_BAR_CHART: char = '\u{F211}';
    pub const GROUPS: char = '\u{F233}';
    pub const GROUPS_2: char = '\u{F8DF}';
    pub const GROUPS_3: char = '\u{F8E0}';
    pub const GUARDIAN: char = '\u{F4C1}';
    pub const GYNECOLOGY: char = '\u{E0F4}';
    pub const H_MOBILEDATA: char = '\u{F018}';
    pub const H_MOBILEDATA_BADGE: char = '\u{F7E0}';
    pub const H_PLUS_MOBILEDATA: char = '\u{F019}';
    pub const H_PLUS_MOBILEDATA_BADGE: char = '\u{F7DF}';
    pub const HAIL: char = '\u{E9B1}';
    pub const HALLWAY: char = '\u{E6F8}';
    pub const HANAMI_DANGO: char = '\u{F23F}';
    pub const HAND_BONES: char = '\u{F894}';
    pub const HAND_GESTURE: char = '\u{EF9C}';
    pub const HAND_GESTURE_OFF: char = '\u{F3F3}';
    pub const HAND_MEAL: char = '\u{F294}';
    pub const HAND_PACKAGE: char = '\u{F293}';
    pub const HANDHELD_CONTROLLER: char = '\u{F4C6}';
    pub const HANDSHAKE: char = '\u{EBCB}';
    pub const HANDWRITING_RECOGNITION: char = '\u{EB02}';
    pub const HANDYMAN: char = '\u{F10B}';
    pub const HANGOUT_VIDEO: char = '\u{E0C1}';
    pub const HANGOUT_VIDEO_OFF: char = '\u{E0C2}';
    pub const HARD_DISK: char = '\u{F3DA}';
    pub const HARD_DRIVE: char = '\u{F80E}';
    pub const HARD_DRIVE_2: char = '\u{F7A4}';
    pub const HARDWARE: char = '\u{EA59}';
    pub const HD: char = '\u{E052}';
    pub const HDR_AUTO: char = '\u{F01A}';
    pub const HDR_AUTO_SELECT: char = '\u{F01B}';
    pub const HDR_ENHANCED_SELECT: char = '\u{EF51}';
    pub const HDR_OFF: char = '\u{E3ED}';
    pub const HDR_OFF_SELECT: char = '\u{F01C}';
    pub const HDR_ON: char = '\u{E3EE}';
    pub const HDR_ON_SELECT: char = '\u{F01D}';
    pub const HDR_PLUS: char = '\u{F01E}';
    pub const HDR_PLUS_OFF: char = '\u{E3EF}';
    pub const HDR_STRONG: char = '\u{E3F1}';
    pub const HDR_WEAK: char = '\u{E3F2}';
    pub const HEAD_MOUNTED_DEVICE: char = '\u{F4C5}';
    pub const HEADPHONES: char = '\u{F01F}';
    pub const HEADPHONES_BATTERY: char = '\u{F020}';
    pub const HEADSET: char = '\u{F01F}';
    pub const HEADSET_MIC: char = '\u{E311}';
    pub const HEADSET_OFF: char = '\u{E33A}';
    pub const HEALING: char = '\u{E3F3}';
    pub const HEALTH_AND_BEAUTY: char = '\u{EF9D}';
    pub const HEALTH_AND_SAFETY: char = '\u{E1D5}';
    pub const HEALTH_CROSS: char = '\u{F2C3}';
    pub const HEALTH_METRICS: char = '\u{F6E2}';
    pub const HEAP_SNAPSHOT_LARGE: char = '\u{F76E}';
    pub const HEAP_SNAPSHOT_MULTIPLE: char = '\u{F76D}';
    pub const HEAP_SNAPSHOT_THUMBNAIL: char = '\u{F76C}';
    pub const HEARING: char = '\u{E023}';
    pub const HEARING_AID: char = '\u{F464}';
    pub const HEARING_AID_DISABLED: char = '\u{F3B0}';
    pub const HEARING_AID_DISABLED_LEFT: char = '\u{F2EC}';
    pub const HEARING_AID_LEFT: char = '\u{F2ED}';
    pub const HEARING_DISABLED: char = '\u{F104}';
    pub const HEART_BROKEN: char = '\u{EAC2}';
    pub const HEART_CHECK: char = '\u{F60A}';
    pub const HEART_MINUS: char = '\u{F883}';
    pub const HEART_PLUS: char = '\u{F884}';
    pub const HEART_SMILE: char = '\u{F292}';
    pub const HEAT: char = '\u{F537}';
    pub const HEAT_PUMP: char = '\u{EC18}';
    pub const HEAT_PUMP_BALANCE: char = '\u{E27E}';
    pub const HEIGHT: char = '\u{EA16}';
    pub const HELICOPTER: char = '\u{F60C}';
    pub const HELP: char = '\u{E8FD}';
    pub const HELP_CENTER: char = '\u{F1C0}';
    pub const HELP_CLINIC: char = '\u{F810}';
    pub const HELP_OUTLINE: char = '\u{E8FD}';
    pub const HEMATOLOGY: char = '\u{E0F6}';
    pub const HEVC: char = '\u{F021}';
    pub const HEXAGON: char = '\u{EB39}';
    pub const HIDE: char = '\u{EF9E}';
    pub const HIDE_IMAGE: char = '\u{F022}';
    pub const HIDE_SOURCE: char = '\u{F023}';
    pub const HIGH_CHAIR: char = '\u{F29A}';
    pub const HIGH_DENSITY: char = '\u{F79C}';
    pub const HIGH_QUALITY: char = '\u{E024}';
    pub const HIGH_QUALITY_OFF: char = '\u{FFED6}';
    pub const HIGH_RES: char = '\u{F54B}';
    pub const HIGHLIGHT: char = '\u{E25F}';
    pub const HIGHLIGHT_ALT: char = '\u{EF52}';
    pub const HIGHLIGHT_KEYBOARD_FOCUS: char = '\u{F510}';
    pub const HIGHLIGHT_MOUSE_CURSOR: char = '\u{F511}';
    pub const HIGHLIGHT_OFF: char = '\u{E888}';
    pub const HIGHLIGHT_TEXT_CURSOR: char = '\u{F512}';
    pub const HIGHLIGHTER_SIZE_1: char = '\u{F76B}';
    pub const HIGHLIGHTER_SIZE_2: char = '\u{F76A}';
    pub const HIGHLIGHTER_SIZE_3: char = '\u{F769}';
    pub const HIGHLIGHTER_SIZE_4: char = '\u{F768}';
    pub const HIGHLIGHTER_SIZE_5: char = '\u{F767}';
    pub const HIKING: char = '\u{E50A}';
    pub const HISTORY: char = '\u{E8B3}';
    pub const HISTORY_2: char = '\u{F3E6}';
    pub const HISTORY_EDU: char = '\u{EA3E}';
    pub const HISTORY_OFF: char = '\u{F4DA}';
    pub const HISTORY_TOGGLE_OFF: char = '\u{F17D}';
    pub const HIVE: char = '\u{EAA6}';
    pub const HLS: char = '\u{EB8A}';
    pub const HLS_OFF: char = '\u{EB8C}';
    pub const HOLIDAY_VILLAGE: char = '\u{E58A}';
    pub const HOME: char = '\u{E9B2}';
    pub const HOME_AND_GARDEN: char = '\u{EF9F}';
    pub const HOME_APP_LOGO: char = '\u{E295}';
    pub const HOME_FILLED: char = '\u{E9B2}';
    pub const HOME_HEALTH: char = '\u{E4B9}';
    pub const HOME_IMPROVEMENT_AND_TOOLS: char = '\u{EFA0}';
    pub const HOME_IOT_DEVICE: char = '\u{E283}';
    pub const HOME_MAX: char = '\u{F024}';
    pub const HOME_MAX_DOTS: char = '\u{E849}';
    pub const HOME_MINI: char = '\u{F025}';
    pub const HOME_PIN: char = '\u{F14D}';
    pub const HOME_REPAIR_SERVICE: char = '\u{F100}';
    pub const HOME_SPEAKER: char = '\u{F11C}';
    pub const HOME_STORAGE: char = '\u{F86C}';
    pub const HOME_STORAGE_GEAR: char = '\u{FFF7E}';
    pub const HOME_WORK: char = '\u{F030}';
    pub const HORIZONTAL_ALIGN_CENTER: char = '\u{FFF9C}';
    pub const HORIZONTAL_ALIGN_LEFT: char = '\u{FFF9B}';
    pub const HORIZONTAL_ALIGN_RIGHT: char = '\u{FFF9A}';
    pub const HORIZONTAL_DISTRIBUTE: char = '\u{E014}';
    pub const HORIZONTAL_RULE: char = '\u{F108}';
    pub const HORIZONTAL_SPLIT: char = '\u{E947}';
    pub const HOST: char = '\u{F3D9}';
    pub const HOT_TUB: char = '\u{EB46}';
    pub const HOTEL: char = '\u{E549}';
    pub const HOTEL_CLASS: char = '\u{E743}';
    pub const HOURGLASS: char = '\u{EBFF}';
    pub const HOURGLASS_ARROW_DOWN: char = '\u{F37E}';
    pub const HOURGLASS_ARROW_UP: char = '\u{F37D}';
    pub const HOURGLASS_BOTTOM: char = '\u{EA5C}';
    pub const HOURGLASS_CHECK: char = '\u{FFFED}';
    pub const HOURGLASS_DISABLED: char = '\u{EF53}';
    pub const HOURGLASS_EMPTY: char = '\u{E88B}';
    pub const HOURGLASS_FULL: char = '\u{E88C}';
    pub const HOURGLASS_PAUSE: char = '\u{F38C}';
    pub const HOURGLASS_TOP: char = '\u{EA5B}';
    pub const HOUSE: char = '\u{EA44}';
    pub const HOUSE_SIDING: char = '\u{F202}';
    pub const HOUSE_WITH_SHIELD: char = '\u{E786}';
    pub const HOUSEBOAT: char = '\u{E584}';
    pub const HOUSEHOLD_SUPPLIES: char = '\u{EFA1}';
    pub const HOV: char = '\u{F475}';
    pub const HOW_TO_REG: char = '\u{E174}';
    pub const HOW_TO_VOTE: char = '\u{E175}';
    pub const HR_RESTING: char = '\u{F6BA}';
    pub const HTML: char = '\u{EB7E}';
    pub const HTTP: char = '\u{E902}';
    pub const HTTPS: char = '\u{E899}';
    pub const HUB: char = '\u{E9F4}';
    pub const HUMERUS: char = '\u{F895}';
    pub const HUMERUS_ALT: char = '\u{F896}';
    pub const HUMIDITY_HIGH: char = '\u{F163}';
    pub const HUMIDITY_INDOOR: char = '\u{F558}';
    pub const HUMIDITY_LOW: char = '\u{F164}';
    pub const HUMIDITY_MID: char = '\u{F165}';
    pub const HUMIDITY_PERCENTAGE: char = '\u{F87E}';
    pub const HVAC: char = '\u{F10E}';
    pub const HVAC_MAX_DEFROST: char = '\u{F332}';
    pub const ICE_SKATING: char = '\u{E50B}';
    pub const ICECREAM: char = '\u{EA69}';
    pub const ID_CARD: char = '\u{F4CA}';
    pub const ID_CARD_2: char = '\u{FFEEA}';
    pub const IDENTITY_AWARE_PROXY: char = '\u{E2DD}';
    pub const IDENTITY_PLATFORM: char = '\u{EBB7}';
    pub const IFL: char = '\u{E025}';
    pub const IFRAME: char = '\u{F71B}';
    pub const IFRAME_OFF: char = '\u{F71C}';
    pub const IMAGE: char = '\u{E3F4}';
    pub const IMAGE_ARROW_UP: char = '\u{F317}';
    pub const IMAGE_ASPECT_RATIO: char = '\u{E6A6}';
    pub const IMAGE_INSET: char = '\u{F247}';
    pub const IMAGE_NOT_SUPPORTED: char = '\u{F116}';
    pub const IMAGE_SEARCH: char = '\u{E43F}';
    pub const IMAGESEARCH_ROLLER: char = '\u{E9B4}';
    pub const IMAGESMODE: char = '\u{EFA2}';
    pub const IMMUNOLOGY: char = '\u{E0FB}';
    pub const IMPORT_CONTACTS: char = '\u{E0E0}';
    pub const IMPORT_EXPORT: char = '\u{E8D5}';
    pub const IMPORTANT_DEVICES: char = '\u{E912}';
    pub const IN_HOME_MODE: char = '\u{E833}';
    pub const INACTIVE_ORDER: char = '\u{E0FC}';
    pub const INBOX: char = '\u{E156}';
    pub const INBOX_CUSTOMIZE: char = '\u{F859}';
    pub const INBOX_TEXT: char = '\u{F399}';
    pub const INBOX_TEXT_ASTERISK: char = '\u{F360}';
    pub const INBOX_TEXT_PERSON: char = '\u{F35E}';
    pub const INBOX_TEXT_SHARE: char = '\u{F35C}';
    pub const INCOMPLETE_CIRCLE: char = '\u{E79B}';
    pub const INDETERMINATE_CHECK_BOX: char = '\u{E909}';
    pub const INDETERMINATE_QUESTION_BOX: char = '\u{F56D}';
    pub const INFO: char = '\u{E88E}';
    pub const INFO_I: char = '\u{F59B}';
    pub const INFRARED: char = '\u{F87C}';
    pub const INK_ERASER: char = '\u{E6D0}';
    pub const INK_ERASER_OFF: char = '\u{E7E3}';
    pub const INK_HIGHLIGHTER: char = '\u{E6D1}';
    pub const INK_HIGHLIGHTER_MOVE: char = '\u{F524}';
    pub const INK_HIGHLIGHTER_OFF: char = '\u{FFF14}';
    pub const INK_MARKER: char = '\u{E6D2}';
    pub const INK_PEN: char = '\u{E6D3}';
    pub const INK_SELECTION: char = '\u{EF52}';
    pub const INPATIENT: char = '\u{E0FE}';
    pub const INPUT: char = '\u{E890}';
    pub const INPUT_CIRCLE: char = '\u{F71A}';
    pub const INSERT_CHART: char = '\u{F0CC}';
    pub const INSERT_CHART_FILLED: char = '\u{F0CC}';
    pub const INSERT_CHART_OUTLINED: char = '\u{F0CC}';
    pub const INSERT_COMMENT: char = '\u{E24C}';
    pub const INSERT_DRIVE_FILE: char = '\u{E66D}';
    pub const INSERT_EMOTICON: char = '\u{EA22}';
    pub const INSERT_INVITATION: char = '\u{E878}';
    pub const INSERT_LINK: char = '\u{E250}';
    pub const INSERT_PAGE_BREAK: char = '\u{EACA}';
    pub const INSERT_PHOTO: char = '\u{E3F4}';
    pub const INSERT_TEXT: char = '\u{F827}';
    pub const INSIGHTS: char = '\u{F092}';
    pub const INSTALL_DESKTOP: char = '\u{EB71}';
    pub const INSTALL_MOBILE: char = '\u{F2CD}';
    pub const INSTANT_MIX: char = '\u{E026}';
    pub const INTEGRATION_INSTRUCTIONS: char = '\u{EF54}';
    pub const INTERACTIVE_SPACE: char = '\u{F7FF}';
    pub const INTERESTS: char = '\u{E7C8}';
    pub const INTERPRETER_MODE: char = '\u{E83B}';
    pub const INVENTORY: char = '\u{E179}';
    pub const INVENTORY_2: char = '\u{E1A1}';
    pub const INVERT_COLORS: char = '\u{E891}';
    pub const INVERT_COLORS_OFF: char = '\u{E0C4}';
    pub const IOS: char = '\u{E027}';
    pub const IOS_SHARE: char = '\u{E6B8}';
    pub const IRON: char = '\u{E583}';
    pub const ISO: char = '\u{E3F6}';
    pub const JAMBOARD_KIOSK: char = '\u{E9B5}';
    pub const JAPANESE_CURRY: char = '\u{F284}';
    pub const JAPANESE_FLAG: char = '\u{F283}';
    pub const JAVASCRIPT: char = '\u{EB7C}';
    pub const JEWELRY: char = '\u{FFEDB}';
    pub const JOIN: char = '\u{F84F}';
    pub const JOIN_FULL: char = '\u{F84F}';
    pub const JOIN_INNER: char = '\u{EAF4}';
    pub const JOIN_LEFT: char = '\u{EAF2}';
    pub const JOIN_RIGHT: char = '\u{EAEA}';
    pub const JOYSTICK: char = '\u{F5EE}';
    pub const JUMP_TO_ELEMENT: char = '\u{F719}';
    pub const KANJI_ALCOHOL: char = '\u{F23E}';
    pub const KAYAKING: char = '\u{E50C}';
    pub const KEBAB_DINING: char = '\u{E842}';
    pub const KEEP: char = '\u{F027}';
    pub const KEEP_OFF: char = '\u{E6F9}';
    pub const KEEP_PIN: char = '\u{F027}';
    pub const KEEP_PUBLIC: char = '\u{F56F}';
    pub const KETTLE: char = '\u{E2B9}';
    pub const KEY: char = '\u{E73C}';
    pub const KEY_OFF: char = '\u{EB84}';
    pub const KEY_VERTICAL: char = '\u{F51A}';
    pub const KEY_VISUALIZER: char = '\u{F199}';
    pub const KEYBOARD: char = '\u{E312}';
    pub const KEYBOARD_ALT: char = '\u{F028}';
    pub const KEYBOARD_ARROW_DOWN: char = '\u{E313}';
    pub const KEYBOARD_ARROW_LEFT: char = '\u{E314}';
    pub const KEYBOARD_ARROW_RIGHT: char = '\u{E315}';
    pub const KEYBOARD_ARROW_UP: char = '\u{E316}';
    pub const KEYBOARD_BACKSPACE: char = '\u{E317}';
    pub const KEYBOARD_CAPSLOCK: char = '\u{E318}';
    pub const KEYBOARD_CAPSLOCK_BADGE: char = '\u{F7DE}';
    pub const KEYBOARD_COMMAND_KEY: char = '\u{EAE7}';
    pub const KEYBOARD_CONTROL_KEY: char = '\u{EAE6}';
    pub const KEYBOARD_DOUBLE_ARROW_DOWN: char = '\u{EAD0}';
    pub const KEYBOARD_DOUBLE_ARROW_LEFT: char = '\u{EAC3}';
    pub const KEYBOARD_DOUBLE_ARROW_RIGHT: char = '\u{EAC9}';
    pub const KEYBOARD_DOUBLE_ARROW_UP: char = '\u{EACF}';
    pub const KEYBOARD_EXTERNAL_INPUT: char = '\u{F7DD}';
    pub const KEYBOARD_FULL: char = '\u{F7DC}';
    pub const KEYBOARD_HIDE: char = '\u{E31A}';
    pub const KEYBOARD_KEYS: char = '\u{F67B}';
    pub const KEYBOARD_LOCK: char = '\u{F492}';
    pub const KEYBOARD_LOCK_OFF: char = '\u{F491}';
    pub const KEYBOARD_OFF: char = '\u{F67A}';
    pub const KEYBOARD_ONSCREEN: char = '\u{F7DB}';
    pub const KEYBOARD_OPTION_KEY: char = '\u{EAE8}';
    pub const KEYBOARD_PREVIOUS_LANGUAGE: char = '\u{F7DA}';
    pub const KEYBOARD_RETURN: char = '\u{E31B}';
    pub const KEYBOARD_TAB: char = '\u{E31C}';
    pub const KEYBOARD_TAB_RTL: char = '\u{EC73}';
    pub const KEYBOARD_VOICE: char = '\u{E31D}';
    pub const KID_STAR: char = '\u{F526}';
    pub const KING_BED: char = '\u{EA45}';
    pub const KITCHEN: char = '\u{EB47}';
    pub const KITESURFING: char = '\u{E50D}';
    pub const LAB_PANEL: char = '\u{E103}';
    pub const LAB_PROFILE: char = '\u{E104}';
    pub const LAB_RESEARCH: char = '\u{F80B}';
    pub const LABEL: char = '\u{E893}';
    pub const LABEL_IMPORTANT: char = '\u{E948}';
    pub const LABEL_IMPORTANT_OUTLINE: char = '\u{E948}';
    pub const LABEL_OFF: char = '\u{E9B6}';
    pub const LABEL_OUTLINE: char = '\u{E893}';
    pub const LABS: char = '\u{E105}';
    pub const LAN: char = '\u{EB2F}';
    pub const LANDSCAPE: char = '\u{E564}';
    pub const LANDSCAPE_2: char = '\u{F4C4}';
    pub const LANDSCAPE_2_EDIT: char = '\u{F310}';
    pub const LANDSCAPE_2_OFF: char = '\u{F4C3}';
    pub const LANDSLIDE: char = '\u{EBD7}';
    pub const LANGUAGE: char = '\u{EA07}';
    pub const LANGUAGE_CHINESE_ARRAY: char = '\u{F766}';
    pub const LANGUAGE_CHINESE_CANGJIE: char = '\u{F765}';
    pub const LANGUAGE_CHINESE_DAYI: char = '\u{F764}';
    pub const LANGUAGE_CHINESE_PINYIN: char = '\u{F763}';
    pub const LANGUAGE_CHINESE_QUICK: char = '\u{F762}';
    pub const LANGUAGE_CHINESE_WUBI: char = '\u{F761}';
    pub const LANGUAGE_FRENCH: char = '\u{F760}';
    pub const LANGUAGE_GB_ENGLISH: char = '\u{F75F}';
    pub const LANGUAGE_INTERNATIONAL: char = '\u{F75E}';
    pub const LANGUAGE_JAPANESE_KANA: char = '\u{F513}';
    pub const LANGUAGE_KOREAN_LATIN: char = '\u{F75D}';
    pub const LANGUAGE_PINYIN: char = '\u{F75C}';
    pub const LANGUAGE_SPANISH: char = '\u{F5E9}';
    pub const LANGUAGE_US: char = '\u{F759}';
    pub const LANGUAGE_US_COLEMAK: char = '\u{F75B}';
    pub const LANGUAGE_US_DVORAK: char = '\u{F75A}';
    pub const LAPS: char = '\u{F6B9}';
    pub const LAPTOP: char = '\u{E31E}';
    pub const LAPTOP_CAR: char = '\u{F3CD}';
    pub const LAPTOP_CHROMEBOOK: char = '\u{E31F}';
    pub const LAPTOP_MAC: char = '\u{E320}';
    pub const LAPTOP_WINDOWS: char = '\u{E321}';
    pub const LASSO_SELECT: char = '\u{EB03}';
    pub const LAST_PAGE: char = '\u{E5DD}';
    pub const LAUNCH: char = '\u{E89E}';
    pub const LAUNDRY: char = '\u{E2A8}';
    pub const LAYERS: char = '\u{E53B}';
    pub const LAYERS_CLEAR: char = '\u{E53C}';
    pub const LDA: char = '\u{E106}';
    pub const LEADERBOARD: char = '\u{F20C}';
    pub const LEAK_ADD: char = '\u{E3F8}';
    pub const LEAK_REMOVE: char = '\u{E3F9}';
    pub const LEFT_CLICK: char = '\u{F718}';
    pub const LEFT_PANEL_CLOSE: char = '\u{F717}';
    pub const LEFT_PANEL_OPEN: char = '\u{F716}';
    pub const LEGEND_TOGGLE: char = '\u{F11B}';
    pub const LENS: char = '\u{E3FA}';
    pub const LENS_BLUR: char = '\u{F029}';
    pub const LETTER_SWITCH: char = '\u{F758}';
    pub const LIBRARY_ADD: char = '\u{E03C}';
    pub const LIBRARY_ADD_CHECK: char = '\u{E9B7}';
    pub const LIBRARY_BOOKS: char = '\u{E02F}';
    pub const LIBRARY_MUSIC: char = '\u{E030}';
    pub const LICENSE: char = '\u{EB04}';
    pub const LIFT_TO_TALK: char = '\u{EFA3}';
    pub const LIGHT: char = '\u{F02A}';
    pub const LIGHT_GROUP: char = '\u{E28B}';
    pub const LIGHT_GROUP_2: char = '\u{FFF76}';
    pub const LIGHT_MODE: char = '\u{E518}';
    pub const LIGHT_MODE_AUTO: char = '\u{FFF00}';
    pub const LIGHT_OFF: char = '\u{E9B8}';
    pub const LIGHTBULB: char = '\u{E90F}';
    pub const LIGHTBULB_2: char = '\u{F3E3}';
    pub const LIGHTBULB_CIRCLE: char = '\u{EBFE}';
    pub const LIGHTBULB_OUTLINE: char = '\u{E90F}';
    pub const LIGHTNING_STAND: char = '\u{EFA4}';
    pub const LIGHTSTRIP: char = '\u{FFF75}';
    pub const LINE_AXIS: char = '\u{EA9A}';
    pub const LINE_CURVE: char = '\u{F757}';
    pub const LINE_END: char = '\u{F826}';
    pub const LINE_END_ARROW: char = '\u{F81D}';
    pub const LINE_END_ARROW_NOTCH: char = '\u{F81C}';
    pub const LINE_END_CIRCLE: char = '\u{F81B}';
    pub const LINE_END_DIAMOND: char = '\u{F81A}';
    pub const LINE_END_SQUARE: char = '\u{F819}';
    pub const LINE_START: char = '\u{F825}';
    pub const LINE_START_ARROW: char = '\u{F818}';
    pub const LINE_START_ARROW_NOTCH: char = '\u{F817}';
    pub const LINE_START_CIRCLE: char = '\u{F816}';
    pub const LINE_START_DIAMOND: char = '\u{F815}';
    pub const LINE_START_SQUARE: char = '\u{F814}';
    pub const LINE_STYLE: char = '\u{E919}';
    pub const LINE_WEIGHT: char = '\u{E91A}';
    pub const LINEAR_SCALE: char = '\u{E260}';
    pub const LINK: char = '\u{E250}';
    pub const LINK_2: char = '\u{FFFB5}';
    pub const LINK_OFF: char = '\u{E16F}';
    pub const LINKED_CAMERA: char = '\u{E438}';
    pub const LINKED_SERVICES: char = '\u{F535}';
    pub const LIPS: char = '\u{EEB2}';
    pub const LIQUOR: char = '\u{EA60}';
    pub const LIST: char = '\u{E896}';
    pub const LIST_2: char = '\u{FFECA}';
    pub const LIST_ALT: char = '\u{E0EE}';
    pub const LIST_ALT_ADD: char = '\u{F756}';
    pub const LIST_ALT_CHECK: char = '\u{F3DE}';
    pub const LIST_ARROW: char = '\u{FFF33}';
    pub const LISTS: char = '\u{E9B9}';
    pub const LIVE_HELP: char = '\u{E0C6}';
    pub const LIVE_TV: char = '\u{E63A}';
    pub const LIVING: char = '\u{F02B}';
    pub const LOCAL_ACTIVITY: char = '\u{E553}';
    pub const LOCAL_AIRPORT: char = '\u{E53D}';
    pub const LOCAL_ATM: char = '\u{E53E}';
    pub const LOCAL_BAR: char = '\u{E540}';
    pub const LOCAL_CAFE: char = '\u{EB44}';
    pub const LOCAL_CAR_WASH: char = '\u{E542}';
    pub const LOCAL_CONVENIENCE_STORE: char = '\u{E543}';
    pub const LOCAL_DINING: char = '\u{E561}';
    pub const LOCAL_DRINK: char = '\u{E544}';
    pub const LOCAL_FIRE_DEPARTMENT: char = '\u{EF55}';
    pub const LOCAL_FLORIST: char = '\u{E545}';
    pub const LOCAL_GAS_STATION: char = '\u{E546}';
    pub const LOCAL_GROCERY_STORE: char = '\u{E8CC}';
    pub const LOCAL_HOSPITAL: char = '\u{E548}';
    pub const LOCAL_HOTEL: char = '\u{E549}';
    pub const LOCAL_LAUNDRY_SERVICE: char = '\u{E54A}';
    pub const LOCAL_LIBRARY: char = '\u{E54B}';
    pub const LOCAL_MALL: char = '\u{E54C}';
    pub const LOCAL_MOVIES: char = '\u{E8DA}';
    pub const LOCAL_OFFER: char = '\u{F05B}';
    pub const LOCAL_PARKING: char = '\u{E54F}';
    pub const LOCAL_PHARMACY: char = '\u{E550}';
    pub const LOCAL_PHONE: char = '\u{F0D4}';
    pub const LOCAL_PIZZA: char = '\u{E552}';
    pub const LOCAL_PLAY: char = '\u{E553}';
    pub const LOCAL_POLICE: char = '\u{EF56}';
    pub const LOCAL_POST_OFFICE: char = '\u{E554}';
    pub const LOCAL_PRINTSHOP: char = '\u{E8AD}';
    pub const LOCAL_SEE: char = '\u{E557}';
    pub const LOCAL_SHIPPING: char = '\u{E558}';
    pub const LOCAL_TAXI: char = '\u{E559}';
    pub const LOCATION_AUTOMATION: char = '\u{F14F}';
    pub const LOCATION_AWAY: char = '\u{F150}';
    pub const LOCATION_CHIP: char = '\u{F850}';
    pub const LOCATION_CITY: char = '\u{E7F1}';
    pub const LOCATION_DISABLED: char = '\u{E1B6}';
    pub const LOCATION_HOME: char = '\u{F152}';
    pub const LOCATION_OFF: char = '\u{E0C7}';
    pub const LOCATION_ON: char = '\u{F1DB}';
    pub const LOCATION_PIN: char = '\u{F1DB}';
    pub const LOCATION_SEARCHING: char = '\u{E1B7}';
    pub const LOCATOR_TAG: char = '\u{F8C1}';
    pub const LOCK: char = '\u{E899}';
    pub const LOCK_CLOCK: char = '\u{EF57}';
    pub const LOCK_OPEN: char = '\u{E898}';
    pub const LOCK_OPEN_CIRCLE: char = '\u{F361}';
    pub const LOCK_OPEN_RIGHT: char = '\u{F656}';
    pub const LOCK_OUTLINE: char = '\u{E899}';
    pub const LOCK_PERSON: char = '\u{F8F3}';
    pub const LOCK_RESET: char = '\u{EADE}';
    pub const LOGIN: char = '\u{EA77}';
    pub const LOGO_DEV: char = '\u{EAD6}';
    pub const LOGOUT: char = '\u{E9BA}';
    pub const LOOKS: char = '\u{E3FC}';
    pub const LOOKS_3: char = '\u{E3FB}';
    pub const LOOKS_4: char = '\u{E3FD}';
    pub const LOOKS_5: char = '\u{E3FE}';
    pub const LOOKS_6: char = '\u{E3FF}';
    pub const LOOKS_ONE: char = '\u{E400}';
    pub const LOOKS_TWO: char = '\u{E401}';
    pub const LOOP: char = '\u{E863}';
    pub const LOUPE: char = '\u{E402}';
    pub const LOW_DENSITY: char = '\u{F79B}';
    pub const LOW_PRIORITY: char = '\u{E16D}';
    pub const LOWERCASE: char = '\u{F48A}';
    pub const LOYALTY: char = '\u{E89A}';
    pub const LTE_MOBILEDATA: char = '\u{F02C}';
    pub const LTE_MOBILEDATA_BADGE: char = '\u{F7D9}';
    pub const LTE_PLUS_MOBILEDATA: char = '\u{F02D}';
    pub const LTE_PLUS_MOBILEDATA_BADGE: char = '\u{F7D8}';
    pub const LUGGAGE: char = '\u{F235}';
    pub const LUNCH_DINING: char = '\u{EA61}';
    pub const LYRICS: char = '\u{EC0B}';
    pub const MACRO_AUTO: char = '\u{F6F2}';
    pub const MACRO_OFF: char = '\u{F8D2}';
    pub const MAGIC_BUTTON: char = '\u{F136}';
    pub const MAGIC_EXCHANGE: char = '\u{F7F4}';
    pub const MAGIC_TETHER: char = '\u{F7D7}';
    pub const MAGNIFICATION_LARGE: char = '\u{F83D}';
    pub const MAGNIFICATION_SMALL: char = '\u{F83C}';
    pub const MAGNIFY_DOCKED: char = '\u{F7D6}';
    pub const MAGNIFY_FULLSCREEN: char = '\u{F7D5}';
    pub const MAIL: char = '\u{E159}';
    pub const MAIL_ASTERISK: char = '\u{EEF4}';
    pub const MAIL_LOCK: char = '\u{EC0A}';
    pub const MAIL_OFF: char = '\u{F48B}';
    pub const MAIL_OUTLINE: char = '\u{E159}';
    pub const MAIL_SHIELD: char = '\u{F249}';
    pub const MALE: char = '\u{E58E}';
    pub const MAN: char = '\u{E4EB}';
    pub const MAN_2: char = '\u{F8E1}';
    pub const MAN_3: char = '\u{F8E2}';
    pub const MAN_4: char = '\u{F8E3}';
    pub const MANAGE_ACCOUNTS: char = '\u{F02E}';
    pub const MANAGE_HISTORY: char = '\u{EBE7}';
    pub const MANAGE_SEARCH: char = '\u{F02F}';
    pub const MANGA: char = '\u{F5E3}';
    pub const MANUFACTURING: char = '\u{E726}';
    pub const MAP: char = '\u{E55B}';
    pub const MAP_PIN_HEART: char = '\u{F298}';
    pub const MAP_PIN_REVIEW: char = '\u{F297}';
    pub const MAP_SEARCH: char = '\u{F3CA}';
    pub const MAPS_HOME_WORK: char = '\u{F030}';
    pub const MAPS_UGC: char = '\u{EF58}';
    pub const MARGIN: char = '\u{E9BB}';
    pub const MARK_AS_UNREAD: char = '\u{E9BC}';
    pub const MARK_CHAT_READ: char = '\u{F18B}';
    pub const MARK_CHAT_UNREAD: char = '\u{F189}';
    pub const MARK_EMAIL_READ: char = '\u{F18C}';
    pub const MARK_EMAIL_UNREAD: char = '\u{F18A}';
    pub const MARK_UNREAD_CHAT_ALT: char = '\u{EB9D}';
    pub const MARKDOWN: char = '\u{F552}';
    pub const MARKDOWN_COPY: char = '\u{F553}';
    pub const MARKDOWN_PASTE: char = '\u{F554}';
    pub const MARKUNREAD: char = '\u{E159}';
    pub const MARKUNREAD_MAILBOX: char = '\u{E89B}';
    pub const MASKED_TRANSITIONS: char = '\u{E72E}';
    pub const MASKED_TRANSITIONS_ADD: char = '\u{F42B}';
    pub const MASKS: char = '\u{F218}';
    pub const MASSAGE: char = '\u{F2C2}';
    pub const MATCH_CASE: char = '\u{F6F1}';
    pub const MATCH_CASE_OFF: char = '\u{F36F}';
    pub const MATCH_WORD: char = '\u{F6F0}';
    pub const MATTER: char = '\u{E907}';
    pub const MAXIMIZE: char = '\u{E930}';
    pub const MEAL_DINNER: char = '\u{F23D}';
    pub const MEAL_LUNCH: char = '\u{F23C}';
    pub const MEASURING_TAPE: char = '\u{F6AF}';
    pub const MEDIA_BLUETOOTH_OFF: char = '\u{F031}';
    pub const MEDIA_BLUETOOTH_ON: char = '\u{F032}';
    pub const MEDIA_LINK: char = '\u{F83F}';
    pub const MEDIA_OUTPUT: char = '\u{F4F2}';
    pub const MEDIA_OUTPUT_OFF: char = '\u{F4F3}';
    pub const MEDIATION: char = '\u{EFA7}';
    pub const MEDICAL_INFORMATION: char = '\u{EBED}';
    pub const MEDICAL_MASK: char = '\u{F80A}';
    pub const MEDICAL_SERVICES: char = '\u{F109}';
    pub const MEDICATION: char = '\u{F033}';
    pub const MEDICATION_LIQUID: char = '\u{EA87}';
    pub const MEETING_ROOM: char = '\u{EB4F}';
    pub const MEMORY: char = '\u{E322}';
    pub const MEMORY_ALT: char = '\u{F7A3}';
    pub const MENSTRUAL_HEALTH: char = '\u{F6E1}';
    pub const MENU: char = '\u{E5D2}';
    pub const MENU_BOOK: char = '\u{EA19}';
    pub const MENU_BOOK_2: char = '\u{F291}';
    pub const MENU_OPEN: char = '\u{E9BD}';
    pub const MERGE: char = '\u{EB98}';
    pub const MERGE_TYPE: char = '\u{E252}';
    pub const MESSAGE: char = '\u{E0C9}';
    pub const METABOLISM: char = '\u{E10B}';
    pub const METRO: char = '\u{F474}';
    pub const MFG_NEST_YALE_LOCK: char = '\u{F11D}';
    pub const MIC: char = '\u{E31D}';
    pub const MIC_ALERT: char = '\u{F392}';
    pub const MIC_DOUBLE: char = '\u{F5D1}';
    pub const MIC_EXTERNAL_OFF: char = '\u{EF59}';
    pub const MIC_EXTERNAL_ON: char = '\u{EF5A}';
    pub const MIC_GEAR: char = '\u{EEBA}';
    pub const MIC_NONE: char = '\u{E31D}';
    pub const MIC_OFF: char = '\u{E02B}';
    pub const MICROBIOLOGY: char = '\u{E10C}';
    pub const MICROWAVE: char = '\u{F204}';
    pub const MICROWAVE_GEN: char = '\u{E847}';
    pub const MILITARY_TECH: char = '\u{EA3F}';
    pub const MIMO: char = '\u{E9BE}';
    pub const MIMO_DISCONNECT: char = '\u{E9BF}';
    pub const MINDFULNESS: char = '\u{F6E0}';
    pub const MINIMIZE: char = '\u{E931}';
    pub const MINOR_CRASH: char = '\u{EBF1}';
    pub const MINTMARK: char = '\u{EFA9}';
    pub const MISSED_VIDEO_CALL: char = '\u{F0CE}';
    pub const MISSED_VIDEO_CALL_FILLED: char = '\u{F0CE}';
    pub const MISSING_CONTROLLER: char = '\u{E701}';
    pub const MIST: char = '\u{E188}';
    pub const MITRE: char = '\u{F547}';
    pub const MIXTURE_MED: char = '\u{E4C8}';
    pub const MMS: char = '\u{E618}';
    pub const MOBILE: char = '\u{E7BA}';
    pub const MOBILE_2: char = '\u{F2DB}';
    pub const MOBILE_3: char = '\u{F2DA}';
    pub const MOBILE_ALERT: char = '\u{F2D3}';
    pub const MOBILE_ARROW_DOWN: char = '\u{F2CD}';
    pub const MOBILE_ARROW_RIGHT: char = '\u{F2D2}';
    pub const MOBILE_ARROW_UP_RIGHT: char = '\u{F2B9}';
    pub const MOBILE_BLOCK: char = '\u{F2E5}';
    pub const MOBILE_CAMERA: char = '\u{F44E}';
    pub const MOBILE_CAMERA_FRONT: char = '\u{F2C9}';
    pub const MOBILE_CAMERA_REAR: char = '\u{F2C8}';
    pub const MOBILE_CANCEL: char = '\u{F2EA}';
    pub const MOBILE_CAST: char = '\u{F2CC}';
    pub const MOBILE_CHARGE: char = '\u{F2E3}';
    pub const MOBILE_CHAT: char = '\u{F79F}';
    pub const MOBILE_CHECK: char = '\u{F073}';
    pub const MOBILE_CODE: char = '\u{F2E2}';
    pub const MOBILE_DOCK: char = '\u{F2E0}';
    pub const MOBILE_DOTS: char = '\u{F2D0}';
    pub const MOBILE_FRIENDLY: char = '\u{F073}';
    pub const MOBILE_GEAR: char = '\u{F2D9}';
    pub const MOBILE_HAND: char = '\u{F323}';
    pub const MOBILE_HAND_LEFT: char = '\u{F313}';
    pub const MOBILE_HAND_LEFT_OFF: char = '\u{F312}';
    pub const MOBILE_HAND_OFF: char = '\u{F314}';
    pub const MOBILE_INFO: char = '\u{F2DC}';
    pub const MOBILE_LANDSCAPE: char = '\u{ED3E}';
    pub const MOBILE_LAYOUT: char = '\u{F2BF}';
    pub const MOBILE_LOCK_LANDSCAPE: char = '\u{F2D8}';
    pub const MOBILE_LOCK_PORTRAIT: char = '\u{F2BE}';
    pub const MOBILE_LOUPE: char = '\u{F322}';
    pub const MOBILE_MENU: char = '\u{F2D1}';
    pub const MOBILE_OFF: char = '\u{E201}';
    pub const MOBILE_QUESTION: char = '\u{F2E1}';
    pub const MOBILE_ROTATE: char = '\u{F2D5}';
    pub const MOBILE_ROTATE_LOCK: char = '\u{F2D6}';
    pub const MOBILE_SCREEN_SHARE: char = '\u{F2DF}';
    pub const MOBILE_SCREENSAVER: char = '\u{F321}';
    pub const MOBILE_SENSOR_HI: char = '\u{F2EF}';
    pub const MOBILE_SENSOR_LO: char = '\u{F2EE}';
    pub const MOBILE_SHARE: char = '\u{F2DF}';
    pub const MOBILE_SHARE_STACK: char = '\u{F2DE}';
    pub const MOBILE_SOUND: char = '\u{F2E8}';
    pub const MOBILE_SOUND_2: char = '\u{F318}';
    pub const MOBILE_SOUND_OFF: char = '\u{F7AA}';
    pub const MOBILE_SPEAKER: char = '\u{F320}';
    pub const MOBILE_TAP: char = '\u{FFEB2}';
    pub const MOBILE_TEXT: char = '\u{F2EB}';
    pub const MOBILE_TEXT_2: char = '\u{F2E6}';
    pub const MOBILE_THEFT: char = '\u{F2A9}';
    pub const MOBILE_TICKET: char = '\u{F2E4}';
    pub const MOBILE_UNLOCK: char = '\u{EEEA}';
    pub const MOBILE_VIBRATE: char = '\u{F2CB}';
    pub const MOBILE_WRENCH: char = '\u{F2B0}';
    pub const MOBILEDATA_ARROWS: char = '\u{FFFA3}';
    pub const MOBILEDATA_OFF: char = '\u{F034}';
    pub const MODE: char = '\u{F097}';
    pub const MODE_COMMENT: char = '\u{E253}';
    pub const MODE_COOL: char = '\u{F166}';
    pub const MODE_COOL_OFF: char = '\u{F167}';
    pub const MODE_DUAL: char = '\u{F557}';
    pub const MODE_EDIT: char = '\u{F097}';
    pub const MODE_EDIT_OUTLINE: char = '\u{F097}';
    pub const MODE_FAN: char = '\u{F168}';
    pub const MODE_FAN_2: char = '\u{FFFD0}';
    pub const MODE_FAN_OFF: char = '\u{EC17}';
    pub const MODE_HEAT: char = '\u{F16A}';
    pub const MODE_HEAT_COOL: char = '\u{F16B}';
    pub const MODE_HEAT_OFF: char = '\u{F16D}';
    pub const MODE_NIGHT: char = '\u{F036}';
    pub const MODE_OF_TRAVEL: char = '\u{E7CE}';
    pub const MODE_OFF_ON: char = '\u{F16F}';
    pub const MODE_STANDBY: char = '\u{F037}';
    pub const MODEL_TRAINING: char = '\u{F0CF}';
    pub const MODELING: char = '\u{F3AA}';
    pub const MONETIZATION_ON: char = '\u{E263}';
    pub const MONEY: char = '\u{E57D}';
    pub const MONEY_BAG: char = '\u{F3EE}';
    pub const MONEY_OFF: char = '\u{F038}';
    pub const MONEY_OFF_CSRED: char = '\u{F038}';
    pub const MONEY_RANGE: char = '\u{F245}';
    pub const MONITOR: char = '\u{EF5B}';
    pub const MONITOR_HEART: char = '\u{EAA2}';
    pub const MONITOR_WEIGHT: char = '\u{F039}';
    pub const MONITOR_WEIGHT_GAIN: char = '\u{F6DF}';
    pub const MONITOR_WEIGHT_LOSS: char = '\u{F6DE}';
    pub const MONITORING: char = '\u{F190}';
    pub const MONOCHROME_PHOTOS: char = '\u{E403}';
    pub const MONORAIL: char = '\u{F473}';
    pub const MOOD: char = '\u{EA22}';
    pub const MOOD_BAD: char = '\u{E7F3}';
    pub const MOOD_HEART: char = '\u{FFFB4}';
    pub const MOON_STARS: char = '\u{F34F}';
    pub const MOP: char = '\u{E28D}';
    pub const MOPED: char = '\u{EB28}';
    pub const MOPED_PACKAGE: char = '\u{F28B}';
    pub const MORE: char = '\u{E619}';
    pub const MORE_DOWN: char = '\u{F196}';
    pub const MORE_HORIZ: char = '\u{E5D3}';
    pub const MORE_TIME: char = '\u{EA5D}';
    pub const MORE_UP: char = '\u{F197}';
    pub const MORE_VERT: char = '\u{E5D4}';
    pub const MOSQUE: char = '\u{EAB2}';
    pub const MOTION_BLUR: char = '\u{F0D0}';
    pub const MOTION_MODE: char = '\u{F842}';
    pub const MOTION_PHOTOS_AUTO: char = '\u{F03A}';
    pub const MOTION_PHOTOS_OFF: char = '\u{E9C0}';
    pub const MOTION_PHOTOS_ON: char = '\u{E9C1}';
    pub const MOTION_PHOTOS_PAUSE: char = '\u{F227}';
    pub const MOTION_PHOTOS_PAUSED: char = '\u{F227}';
    pub const MOTION_PLAY: char = '\u{F40B}';
    pub const MOTION_SENSOR_ACTIVE: char = '\u{E792}';
    pub const MOTION_SENSOR_ALERT: char = '\u{E784}';
    pub const MOTION_SENSOR_IDLE: char = '\u{E783}';
    pub const MOTION_SENSOR_URGENT: char = '\u{E78E}';
    pub const MOTORCYCLE: char = '\u{E91B}';
    pub const MOUNTAIN_FLAG: char = '\u{F5E2}';
    pub const MOUNTAIN_STEAM: char = '\u{F282}';
    pub const MOUSE: char = '\u{E323}';
    pub const MOUSE_LOCK: char = '\u{F490}';
    pub const MOUSE_LOCK_OFF: char = '\u{F48F}';
    pub const MOVE: char = '\u{E740}';
    pub const MOVE_DOWN: char = '\u{EB61}';
    pub const MOVE_GROUP: char = '\u{F715}';
    pub const MOVE_ITEM: char = '\u{F1FF}';
    pub const MOVE_LOCATION: char = '\u{E741}';
    pub const MOVE_SELECTION_DOWN: char = '\u{F714}';
    pub const MOVE_SELECTION_LEFT: char = '\u{F713}';
    pub const MOVE_SELECTION_RIGHT: char = '\u{F712}';
    pub const MOVE_SELECTION_UP: char = '\u{F711}';
    pub const MOVE_TO_INBOX: char = '\u{E168}';
    pub const MOVE_UP: char = '\u{EB64}';
    pub const MOVED_LOCATION: char = '\u{E594}';
    pub const MOVIE: char = '\u{E404}';
    pub const MOVIE_CREATION: char = '\u{E404}';
    pub const MOVIE_EDIT: char = '\u{F840}';
    pub const MOVIE_EDIT_OFF: char = '\u{FFF7D}';
    pub const MOVIE_FILTER: char = '\u{E43A}';
    pub const MOVIE_INFO: char = '\u{E02D}';
    pub const MOVIE_OFF: char = '\u{F499}';
    pub const MOVIE_SPEAKER: char = '\u{F2A3}';
    pub const MOVING: char = '\u{E501}';
    pub const MOVING_BEDS: char = '\u{E73D}';
    pub const MOVING_MINISTRY: char = '\u{E73E}';
    pub const MP: char = '\u{E9C3}';
    pub const MULTICOOKER: char = '\u{E293}';
    pub const MULTILINE_CHART: char = '\u{E6DF}';
    pub const MULTIMODAL_HAND_EYE: char = '\u{F41B}';
    pub const MULTIPLE_AIRPORTS: char = '\u{EFAB}';
    pub const MULTIPLE_STOP: char = '\u{F1B9}';
    pub const MUSEUM: char = '\u{EA36}';
    pub const MUSIC_CAST: char = '\u{EB1A}';
    pub const MUSIC_HISTORY: char = '\u{F2C1}';
    pub const MUSIC_NOTE: char = '\u{E405}';
    pub const MUSIC_NOTE_2: char = '\u{FFFD8}';
    pub const MUSIC_NOTE_ADD: char = '\u{F391}';
    pub const MUSIC_OFF: char = '\u{E440}';
    pub const MUSIC_VIDEO: char = '\u{E063}';
    pub const MY_LOCATION: char = '\u{E55C}';
    pub const MYSTERY: char = '\u{F5E1}';
    pub const NAT: char = '\u{EF5C}';
    pub const NATURE: char = '\u{E406}';
    pub const NATURE_PEOPLE: char = '\u{E407}';
    pub const NAVIGATE_BEFORE: char = '\u{E5CB}';
    pub const NAVIGATE_NEXT: char = '\u{E5CC}';
    pub const NAVIGATION: char = '\u{E55D}';
    pub const NEAR_ME: char = '\u{E569}';
    pub const NEAR_ME_DISABLED: char = '\u{F1EF}';
    pub const NEARBY: char = '\u{E6B7}';
    pub const NEARBY_ERROR: char = '\u{F03B}';
    pub const NEARBY_OFF: char = '\u{F03C}';
    pub const NEPHROLOGY: char = '\u{E10D}';
    pub const NEST_AUDIO: char = '\u{EBBF}';
    pub const NEST_CAM_FLOODLIGHT: char = '\u{F8B7}';
    pub const NEST_CAM_INDOOR: char = '\u{F11E}';
    pub const NEST_CAM_IQ: char = '\u{F11F}';
    pub const NEST_CAM_IQ_OUTDOOR: char = '\u{F120}';
    pub const NEST_CAM_MAGNET_MOUNT: char = '\u{F8B8}';
    pub const NEST_CAM_OUTDOOR: char = '\u{F121}';
    pub const NEST_CAM_STAND: char = '\u{F8B9}';
    pub const NEST_CAM_WALL_MOUNT: char = '\u{F8BA}';
    pub const NEST_CAM_WIRED_STAND: char = '\u{EC16}';
    pub const NEST_CLOCK_FARSIGHT_ANALOG: char = '\u{F8BB}';
    pub const NEST_CLOCK_FARSIGHT_DIGITAL: char = '\u{F8BC}';
    pub const NEST_CONNECT: char = '\u{F122}';
    pub const NEST_DETECT: char = '\u{F123}';
    pub const NEST_DISPLAY: char = '\u{F124}';
    pub const NEST_DISPLAY_MAX: char = '\u{F125}';
    pub const NEST_DOORBELL_VISITOR: char = '\u{F8BD}';
    pub const NEST_ECO_LEAF: char = '\u{F8BE}';
    pub const NEST_FARSIGHT_COOL: char = '\u{F27D}';
    pub const NEST_FARSIGHT_DUAL: char = '\u{F27C}';
    pub const NEST_FARSIGHT_ECO: char = '\u{F27B}';
    pub const NEST_FARSIGHT_HEAT: char = '\u{F27A}';
    pub const NEST_FARSIGHT_SEASONAL: char = '\u{F279}';
    pub const NEST_FARSIGHT_WEATHER: char = '\u{F8BF}';
    pub const NEST_FOUND_SAVINGS: char = '\u{F8C0}';
    pub const NEST_GALE_WIFI: char = '\u{F579}';
    pub const NEST_HEAT_LINK_E: char = '\u{F126}';
    pub const NEST_HEAT_LINK_GEN_3: char = '\u{F127}';
    pub const NEST_HELLO_DOORBELL: char = '\u{E82C}';
    pub const NEST_LOCATOR_TAG: char = '\u{F8C1}';
    pub const NEST_MINI: char = '\u{E789}';
    pub const NEST_MULTI_ROOM: char = '\u{F8C2}';
    pub const NEST_PROTECT: char = '\u{E68E}';
    pub const NEST_REMOTE: char = '\u{F5DB}';
    pub const NEST_REMOTE_COMFORT_SENSOR: char = '\u{F12A}';
    pub const NEST_SECURE_ALARM: char = '\u{F12B}';
    pub const NEST_SUNBLOCK: char = '\u{F8C3}';
    pub const NEST_TAG: char = '\u{F8C1}';
    pub const NEST_THERMOSTAT: char = '\u{E68F}';
    pub const NEST_THERMOSTAT_E_EU: char = '\u{F12D}';
    pub const NEST_THERMOSTAT_GEN_3: char = '\u{F12E}';
    pub const NEST_THERMOSTAT_SENSOR: char = '\u{F12F}';
    pub const NEST_THERMOSTAT_SENSOR_EU: char = '\u{F130}';
    pub const NEST_THERMOSTAT_ZIRCONIUM_EU: char = '\u{F131}';
    pub const NEST_TRUE_RADIANT: char = '\u{F8C4}';
    pub const NEST_WAKE_ON_APPROACH: char = '\u{F8C5}';
    pub const NEST_WAKE_ON_PRESS: char = '\u{F8C6}';
    pub const NEST_WIFI_GALE: char = '\u{F132}';
    pub const NEST_WIFI_MISTRAL: char = '\u{F133}';
    pub const NEST_WIFI_POINT: char = '\u{F134}';
    pub const NEST_WIFI_POINT_VENTO: char = '\u{F134}';
    pub const NEST_WIFI_PRO: char = '\u{F56B}';
    pub const NEST_WIFI_PRO_2: char = '\u{F56A}';
    pub const NEST_WIFI_ROUTER: char = '\u{F133}';
    pub const NETWORK_CELL: char = '\u{E1B9}';
    pub const NETWORK_CHECK: char = '\u{E640}';
    pub const NETWORK_INTEL_NODE: char = '\u{F371}';
    pub const NETWORK_INTELLIGENCE: char = '\u{EFAC}';
    pub const NETWORK_INTELLIGENCE_HISTORY: char = '\u{F5F6}';
    pub const NETWORK_INTELLIGENCE_UPDATE: char = '\u{F5F5}';
    pub const NETWORK_LOCKED: char = '\u{E61A}';
    pub const NETWORK_MANAGE: char = '\u{F7AB}';
    pub const NETWORK_NODE: char = '\u{F56E}';
    pub const NETWORK_PING: char = '\u{EBCA}';
    pub const NETWORK_WIFI: char = '\u{E1BA}';
    pub const NETWORK_WIFI_1_BAR: char = '\u{EBE4}';
    pub const NETWORK_WIFI_1_BAR_LOCKED: char = '\u{F58F}';
    pub const NETWORK_WIFI_2_BAR: char = '\u{EBD6}';
    pub const NETWORK_WIFI_2_BAR_LOCKED: char = '\u{F58E}';
    pub const NETWORK_WIFI_3_BAR: char = '\u{EBE1}';
    pub const NETWORK_WIFI_3_BAR_LOCKED: char = '\u{F58D}';
    pub const NETWORK_WIFI_LOCKED: char = '\u{F532}';
    pub const NEUROLOGY: char = '\u{E10E}';
    pub const NEW_LABEL: char = '\u{E609}';
    pub const NEW_RELEASES: char = '\u{EF76}';
    pub const NEW_WINDOW: char = '\u{F710}';
    pub const NEWS: char = '\u{E032}';
    pub const NEWSMODE: char = '\u{EFAD}';
    pub const NEWSPAPER: char = '\u{EB81}';
    pub const NEWSSTAND: char = '\u{E9C4}';
    pub const NEXT_PLAN: char = '\u{EF5D}';
    pub const NEXT_WEEK: char = '\u{E16A}';
    pub const NFC: char = '\u{E1BB}';
    pub const NFC_OFF: char = '\u{F369}';
    pub const NIGHT_SHELTER: char = '\u{F1F1}';
    pub const NIGHT_SIGHT_AUTO: char = '\u{F1D7}';
    pub const NIGHT_SIGHT_AUTO_OFF: char = '\u{F1F9}';
    pub const NIGHT_SIGHT_MAX: char = '\u{F6C3}';
    pub const NIGHTLIFE: char = '\u{EA62}';
    pub const NIGHTLIGHT: char = '\u{F03D}';
    pub const NIGHTLIGHT_ROUND: char = '\u{F03D}';
    pub const NIGHTS_STAY: char = '\u{F174}';
    pub const NO_ACCOUNTS: char = '\u{F03E}';
    pub const NO_ADULT_CONTENT: char = '\u{F8FE}';
    pub const NO_BACKPACK: char = '\u{F237}';
    pub const NO_CRASH: char = '\u{EBF0}';
    pub const NO_DRINKS: char = '\u{F1A5}';
    pub const NO_ENCRYPTION: char = '\u{F03F}';
    pub const NO_ENCRYPTION_GMAILERRORRED: char = '\u{F03F}';
    pub const NO_FLASH: char = '\u{F1A6}';
    pub const NO_FOOD: char = '\u{F1A7}';
    pub const NO_LUGGAGE: char = '\u{F23B}';
    pub const NO_MEALS: char = '\u{F1D6}';
    pub const NO_MEETING_ROOM: char = '\u{EB4E}';
    pub const NO_PHOTOGRAPHY: char = '\u{F1A8}';
    pub const NO_SIM: char = '\u{E1CE}';
    pub const NO_SOUND: char = '\u{E710}';
    pub const NO_STROLLER: char = '\u{F1AF}';
    pub const NO_TRANSFER: char = '\u{F1D5}';
    pub const NOISE_AWARE: char = '\u{EBEC}';
    pub const NOISE_CONTROL_OFF: char = '\u{EBF3}';
    pub const NOISE_CONTROL_ON: char = '\u{F8A8}';
    pub const NORDIC_WALKING: char = '\u{E50E}';
    pub const NORTH: char = '\u{F1E0}';
    pub const NORTH_EAST: char = '\u{F1E1}';
    pub const NORTH_WEST: char = '\u{F1E2}';
    pub const NOT_ACCESSIBLE: char = '\u{F0FE}';
    pub const NOT_ACCESSIBLE_FORWARD: char = '\u{F54A}';
    pub const NOT_INTERESTED: char = '\u{F08C}';
    pub const NOT_LISTED_LOCATION: char = '\u{E575}';
    pub const NOT_STARTED: char = '\u{F0D1}';
    pub const NOTE: char = '\u{E66D}';
    pub const NOTE_ADD: char = '\u{E89C}';
    pub const NOTE_ALT: char = '\u{F040}';
    pub const NOTE_STACK: char = '\u{F562}';
    pub const NOTE_STACK_ADD: char = '\u{F563}';
    pub const NOTES: char = '\u{E26C}';
    pub const NOTIFICATION_ADD: char = '\u{E399}';
    pub const NOTIFICATION_AUDIO: char = '\u{EEC1}';
    pub const NOTIFICATION_AUDIO_OFF: char = '\u{EEC0}';
    pub const NOTIFICATION_IMPORTANT: char = '\u{E004}';
    pub const NOTIFICATION_MULTIPLE: char = '\u{E6C2}';
    pub const NOTIFICATION_SETTINGS: char = '\u{F367}';
    pub const NOTIFICATION_SOUND: char = '\u{F353}';
    pub const NOTIFICATIONS: char = '\u{E7F5}';
    pub const NOTIFICATIONS_ACTIVE: char = '\u{E7F7}';
    pub const NOTIFICATIONS_NONE: char = '\u{E7F5}';
    pub const NOTIFICATIONS_OFF: char = '\u{E7F6}';
    pub const NOTIFICATIONS_PAUSED: char = '\u{E7F8}';
    pub const NOTIFICATIONS_UNREAD: char = '\u{F4FE}';
    pub const NUMBERS: char = '\u{EAC7}';
    pub const NUTRITION: char = '\u{E110}';
    pub const ODS: char = '\u{E6E8}';
    pub const ODT: char = '\u{E6E9}';
    pub const OFFLINE_BOLT: char = '\u{E932}';
    pub const OFFLINE_PIN: char = '\u{E90A}';
    pub const OFFLINE_PIN_OFF: char = '\u{F4D0}';
    pub const OFFLINE_SHARE: char = '\u{F2DE}';
    pub const OIL_BARREL: char = '\u{EC15}';
    pub const OKONOMIYAKI: char = '\u{F281}';
    pub const ON_DEVICE_TRAINING: char = '\u{EBFD}';
    pub const ON_HUB_DEVICE: char = '\u{E6C3}';
    pub const ONCOLOGY: char = '\u{E114}';
    pub const ONDEMAND_VIDEO: char = '\u{E63A}';
    pub const ONLINE_PREDICTION: char = '\u{F0EB}';
    pub const ONSEN: char = '\u{F6F8}';
    pub const OPACITY: char = '\u{E91C}';
    pub const OPEN_IN_BROWSER: char = '\u{E89D}';
    pub const OPEN_IN_FULL: char = '\u{F1CE}';
    pub const OPEN_IN_NEW: char = '\u{E89E}';
    pub const OPEN_IN_NEW_DOWN: char = '\u{F70F}';
    pub const OPEN_IN_NEW_OFF: char = '\u{E4F6}';
    pub const OPEN_IN_PHONE: char = '\u{F2D2}';
    pub const OPEN_JAM: char = '\u{EFAE}';
    pub const OPEN_RUN: char = '\u{F4B7}';
    pub const OPEN_WITH: char = '\u{E89F}';
    pub const OPHTHALMOLOGY: char = '\u{E115}';
    pub const ORAL_DISEASE: char = '\u{E116}';
    pub const ORBIT: char = '\u{F426}';
    pub const ORDER_APPROVE: char = '\u{F812}';
    pub const ORDER_PLAY: char = '\u{F811}';
    pub const ORDERS: char = '\u{EB14}';
    pub const ORTHOPEDICS: char = '\u{F897}';
    pub const OTHER_ADMISSION: char = '\u{E47B}';
    pub const OTHER_HOUSES: char = '\u{E58C}';
    pub const OUTBOUND: char = '\u{E1CA}';
    pub const OUTBOX: char = '\u{EF5F}';
    pub const OUTBOX_ALT: char = '\u{EB17}';
    pub const OUTDOOR_GARDEN: char = '\u{E205}';
    pub const OUTDOOR_GRILL: char = '\u{EA47}';
    pub const OUTGOING_MAIL: char = '\u{F0D2}';
    pub const OUTLET: char = '\u{F1D4}';
    pub const OUTLINED_FLAG: char = '\u{F0C6}';
    pub const OUTPATIENT: char = '\u{E118}';
    pub const OUTPATIENT_MED: char = '\u{E119}';
    pub const OUTPUT: char = '\u{EBBE}';
    pub const OUTPUT_CIRCLE: char = '\u{F70E}';
    pub const OVEN: char = '\u{E9C7}';
    pub const OVEN_GEN: char = '\u{E843}';
    pub const OVERVIEW: char = '\u{E4A7}';
    pub const OVERVIEW_KEY: char = '\u{F7D4}';
    pub const OWL: char = '\u{F3B4}';
    pub const OXYGEN_SATURATION: char = '\u{E4DE}';
    pub const P2P: char = '\u{F52A}';
    pub const PACE: char = '\u{F6B8}';
    pub const PACEMAKER: char = '\u{E656}';
    pub const PACKAGE: char = '\u{E48F}';
    pub const PACKAGE_2: char = '\u{F569}';
    pub const PADDING: char = '\u{E9C8}';
    pub const PADEL: char = '\u{F2A7}';
    pub const PAGE_CONTROL: char = '\u{E731}';
    pub const PAGE_FOOTER: char = '\u{F383}';
    pub const PAGE_HEADER: char = '\u{F384}';
    pub const PAGE_INFO: char = '\u{F614}';
    pub const PAGE_MENU_IOS: char = '\u{EEFB}';
    pub const PAGELESS: char = '\u{F509}';
    pub const PAGES: char = '\u{E7F9}';
    pub const PAGEVIEW: char = '\u{E8A0}';
    pub const PAID: char = '\u{F041}';
    pub const PALETTE: char = '\u{E40A}';
    pub const PALLET: char = '\u{F86A}';
    pub const PAN_TOOL: char = '\u{E925}';
    pub const PAN_TOOL_ALT: char = '\u{EBB9}';
    pub const PAN_ZOOM: char = '\u{F655}';
    pub const PANORAMA: char = '\u{E691}';
    pub const PANORAMA_FISH_EYE: char = '\u{E40C}';
    pub const PANORAMA_HORIZONTAL: char = '\u{E40D}';
    pub const PANORAMA_PHOTOSPHERE: char = '\u{E9C9}';
    pub const PANORAMA_VERTICAL: char = '\u{E40E}';
    pub const PANORAMA_WIDE_ANGLE: char = '\u{E40F}';
    pub const PARAGLIDING: char = '\u{E50F}';
    pub const PARENT_CHILD_DINING: char = '\u{F22D}';
    pub const PARK: char = '\u{EA63}';
    pub const PARKING_METER: char = '\u{F28A}';
    pub const PARKING_SIGN: char = '\u{F289}';
    pub const PARKING_VALET: char = '\u{F288}';
    pub const PARTLY_CLOUDY_DAY: char = '\u{F172}';
    pub const PARTLY_CLOUDY_NIGHT: char = '\u{F174}';
    pub const PARTNER_EXCHANGE: char = '\u{F7F9}';
    pub const PARTNER_HEART: char = '\u{EF2E}';
    pub const PARTNER_REPORTS: char = '\u{EFAF}';
    pub const PARTY_MODE: char = '\u{E7FA}';
    pub const PASSKEY: char = '\u{F87F}';
    pub const PASSPORT: char = '\u{EEC4}';
    pub const PASSWORD: char = '\u{F042}';
    pub const PASSWORD_2: char = '\u{F4A9}';
    pub const PASSWORD_2_OFF: char = '\u{F4A8}';
    pub const PATIENT_LIST: char = '\u{E653}';
    pub const PATTERN: char = '\u{F043}';
    pub const PAUSE: char = '\u{E034}';
    pub const PAUSE_CIRCLE: char = '\u{E1A2}';
    pub const PAUSE_CIRCLE_FILLED: char = '\u{E1A2}';
    pub const PAUSE_CIRCLE_OUTLINE: char = '\u{E1A2}';
    pub const PAUSE_PRESENTATION: char = '\u{E0EA}';
    pub const PAYMENT: char = '\u{E8A1}';
    pub const PAYMENT_ARROW_DOWN: char = '\u{F2C0}';
    pub const PAYMENT_CARD: char = '\u{F2A1}';
    pub const PAYMENTS: char = '\u{EF63}';
    pub const PEDAL_BIKE: char = '\u{EB29}';
    pub const PEDIATRICS: char = '\u{E11D}';
    pub const PEN_SIZE_1: char = '\u{F755}';
    pub const PEN_SIZE_2: char = '\u{F754}';
    pub const PEN_SIZE_3: char = '\u{F753}';
    pub const PEN_SIZE_4: char = '\u{F752}';
    pub const PEN_SIZE_5: char = '\u{F751}';
    pub const PENDING: char = '\u{EF64}';
    pub const PENDING_ACTIONS: char = '\u{F1BB}';
    pub const PENTAGON: char = '\u{EB50}';
    pub const PEOPLE: char = '\u{EA21}';
    pub const PEOPLE_ALT: char = '\u{EA21}';
    pub const PEOPLE_OUTLINE: char = '\u{EA21}';
    pub const PEOPLE_SIZE_DECREASE: char = '\u{FFEB1}';
    pub const PEOPLE_SIZE_INCREASE: char = '\u{FFEB0}';
    pub const PERCENT: char = '\u{EB58}';
    pub const PERCENT_DISCOUNT: char = '\u{F244}';
    pub const PERFORMANCE_MAX: char = '\u{E51A}';
    pub const PERGOLA: char = '\u{E203}';
    pub const PERM_CAMERA_MIC: char = '\u{E8A2}';
    pub const PERM_CONTACT_CALENDAR: char = '\u{E8A3}';
    pub const PERM_DATA_SETTING: char = '\u{E8A4}';
    pub const PERM_DEVICE_INFORMATION: char = '\u{F2DC}';
    pub const PERM_IDENTITY: char = '\u{F0D3}';
    pub const PERM_MEDIA: char = '\u{E8A7}';
    pub const PERM_PHONE_MSG: char = '\u{E8A8}';
    pub const PERM_SCAN_WIFI: char = '\u{E8A9}';
    pub const PERSON: char = '\u{F0D3}';
    pub const PERSON_2: char = '\u{F8E4}';
    pub const PERSON_3: char = '\u{F8E5}';
    pub const PERSON_4: char = '\u{F8E6}';
    pub const PERSON_ADD: char = '\u{EA4D}';
    pub const PERSON_ADD_ALT: char = '\u{EA4D}';
    pub const PERSON_ADD_DISABLED: char = '\u{E9CB}';
    pub const PERSON_ALERT: char = '\u{F567}';
    pub const PERSON_APRON: char = '\u{F5A3}';
    pub const PERSON_BOOK: char = '\u{F5E8}';
    pub const PERSON_CANCEL: char = '\u{F566}';
    pub const PERSON_CELEBRATE: char = '\u{F7FE}';
    pub const PERSON_CHECK: char = '\u{F565}';
    pub const PERSON_EDIT: char = '\u{F4FA}';
    pub const PERSON_FILLED: char = '\u{F0D3}';
    pub const PERSON_HEART: char = '\u{F290}';
    pub const PERSON_OFF: char = '\u{E510}';
    pub const PERSON_OUTLINE: char = '\u{F0D3}';
    pub const PERSON_PIN: char = '\u{E55A}';
    pub const PERSON_PIN_CIRCLE: char = '\u{E56A}';
    pub const PERSON_PLAY: char = '\u{F7FD}';
    pub const PERSON_RAISED_HAND: char = '\u{F59A}';
    pub const PERSON_REMOVE: char = '\u{EF66}';
    pub const PERSON_SEARCH: char = '\u{F106}';
    pub const PERSON_SHIELD: char = '\u{E384}';
    pub const PERSON_TEXT: char = '\u{EEBD}';
    pub const PERSONAL_BAG: char = '\u{EB0E}';
    pub const PERSONAL_BAG_OFF: char = '\u{EB0F}';
    pub const PERSONAL_BAG_QUESTION: char = '\u{EB10}';
    pub const PERSONAL_INJURY: char = '\u{E6DA}';
    pub const PERSONAL_PLACES: char = '\u{E703}';
    pub const PERSONAL_VIDEO: char = '\u{E63B}';
    pub const PEST_CONTROL: char = '\u{F0FA}';
    pub const PEST_CONTROL_RODENT: char = '\u{F0FD}';
    pub const PET_SUPPLIES: char = '\u{EFB1}';
    pub const PETS: char = '\u{E91D}';
    pub const PHISHING: char = '\u{EAD7}';
    pub const PHONE: char = '\u{F0D4}';
    pub const PHONE_ALT: char = '\u{F0D4}';
    pub const PHONE_ANDROID: char = '\u{F2DB}';
    pub const PHONE_BLUETOOTH_SPEAKER: char = '\u{E61B}';
    pub const PHONE_CALLBACK: char = '\u{E649}';
    pub const PHONE_CANCEL: char = '\u{FFF9D}';
    pub const PHONE_DISABLED: char = '\u{E9CC}';
    pub const PHONE_ENABLED: char = '\u{E9CD}';
    pub const PHONE_FORWARDED: char = '\u{E61C}';
    pub const PHONE_IN_TALK: char = '\u{E61D}';
    pub const PHONE_IPHONE: char = '\u{F2DA}';
    pub const PHONE_LOCKED: char = '\u{E61E}';
    pub const PHONE_MISSED: char = '\u{E61F}';
    pub const PHONE_PAUSED: char = '\u{E620}';
    pub const PHONELINK: char = '\u{E326}';
    pub const PHONELINK_ERASE: char = '\u{F2EA}';
    pub const PHONELINK_LOCK: char = '\u{F2BE}';
    pub const PHONELINK_OFF: char = '\u{F7A5}';
    pub const PHONELINK_RING: char = '\u{F2E8}';
    pub const PHONELINK_RING_OFF: char = '\u{F7AA}';
    pub const PHONELINK_SETUP: char = '\u{F2D9}';
    pub const PHOTO: char = '\u{E693}';
    pub const PHOTO_ALBUM: char = '\u{E411}';
    pub const PHOTO_AUTO_MERGE: char = '\u{F530}';
    pub const PHOTO_CAMERA: char = '\u{E412}';
    pub const PHOTO_CAMERA_BACK: char = '\u{EF68}';
    pub const PHOTO_CAMERA_FRONT: char = '\u{EF69}';
    pub const PHOTO_FILTER: char = '\u{E43B}';
    pub const PHOTO_FRAME: char = '\u{F0D9}';
    pub const PHOTO_LIBRARY: char = '\u{E413}';
    pub const PHOTO_PRINTS: char = '\u{EFB2}';
    pub const PHOTO_SIZE_SELECT_ACTUAL: char = '\u{E693}';
    pub const PHOTO_SIZE_SELECT_LARGE: char = '\u{E433}';
    pub const PHOTO_SIZE_SELECT_SMALL: char = '\u{E434}';
    pub const PHP: char = '\u{EB8F}';
    pub const PHYSICAL_THERAPY: char = '\u{E11E}';
    pub const PIANO: char = '\u{E521}';
    pub const PIANO_OFF: char = '\u{E520}';
    pub const PICKLEBALL: char = '\u{F2A6}';
    pub const PICTURE_AS_PDF: char = '\u{E415}';
    pub const PICTURE_IN_PICTURE: char = '\u{E8AA}';
    pub const PICTURE_IN_PICTURE_ALT: char = '\u{E911}';
    pub const PICTURE_IN_PICTURE_CENTER: char = '\u{F550}';
    pub const PICTURE_IN_PICTURE_LARGE: char = '\u{F54F}';
    pub const PICTURE_IN_PICTURE_MEDIUM: char = '\u{F54E}';
    pub const PICTURE_IN_PICTURE_MOBILE: char = '\u{F517}';
    pub const PICTURE_IN_PICTURE_OFF: char = '\u{F52F}';
    pub const PICTURE_IN_PICTURE_SMALL: char = '\u{F54D}';
    pub const PIE_CHART: char = '\u{F0DA}';
    pub const PIE_CHART_FILLED: char = '\u{F0DA}';
    pub const PIE_CHART_OUTLINE: char = '\u{F0DA}';
    pub const PIE_CHART_OUTLINED: char = '\u{F0DA}';
    pub const PILL: char = '\u{E11F}';
    pub const PILL_OFF: char = '\u{F809}';
    pub const PIN: char = '\u{F045}';
    pub const PIN_DROP: char = '\u{E55E}';
    pub const PIN_END: char = '\u{E767}';
    pub const PIN_HISTORY: char = '\u{FFF2E}';
    pub const PIN_INVOKE: char = '\u{E763}';
    pub const PIN_ROAD: char = '\u{FFF2D}';
    pub const PIN_ROAD_2: char = '\u{FFEEB}';
    pub const PINBOARD: char = '\u{F3AB}';
    pub const PINBOARD_UNREAD: char = '\u{F3AC}';
    pub const PINCH: char = '\u{EB38}';
    pub const PINCH_ZOOM_IN: char = '\u{F1FA}';
    pub const PINCH_ZOOM_OUT: char = '\u{F1FB}';
    pub const PIP: char = '\u{F64D}';
    pub const PIP_EXIT: char = '\u{F70D}';
    pub const PIVOT_TABLE_CHART: char = '\u{E9CE}';
    pub const PLACE: char = '\u{F1DB}';
    pub const PLACE_ITEM: char = '\u{F1F0}';
    pub const PLAGIARISM: char = '\u{EA5A}';
    pub const PLANE_CONTRAILS: char = '\u{F2AC}';
    pub const PLANET: char = '\u{F387}';
    pub const PLANNER_BANNER_AD_PT: char = '\u{E692}';
    pub const PLANNER_REVIEW: char = '\u{E694}';
    pub const PLAY_ARROW: char = '\u{E037}';
    pub const PLAY_CIRCLE: char = '\u{E1C4}';
    pub const PLAY_DISABLED: char = '\u{EF6A}';
    pub const PLAY_FOR_WORK: char = '\u{E906}';
    pub const PLAY_LESSON: char = '\u{F047}';
    pub const PLAY_MUSIC: char = '\u{E6EE}';
    pub const PLAY_PAUSE: char = '\u{F137}';
    pub const PLAY_SHAPES: char = '\u{F7FC}';
    pub const PLAYGROUND: char = '\u{F28E}';
    pub const PLAYGROUND_2: char = '\u{F28F}';
    pub const PLAYING_CARDS: char = '\u{F5DC}';
    pub const PLAYLIST_ADD: char = '\u{E03B}';
    pub const PLAYLIST_ADD_CHECK: char = '\u{E065}';
    pub const PLAYLIST_ADD_CHECK_CIRCLE: char = '\u{E7E6}';
    pub const PLAYLIST_ADD_CIRCLE: char = '\u{E7E5}';
    pub const PLAYLIST_PLAY: char = '\u{E05F}';
    pub const PLAYLIST_REMOVE: char = '\u{EB80}';
    pub const PLUG_CONNECT: char = '\u{F35A}';
    pub const PLUMBING: char = '\u{F107}';
    pub const PLUS_ONE: char = '\u{E800}';
    pub const PODCASTS: char = '\u{F048}';
    pub const PODIATRY: char = '\u{E120}';
    pub const PODIUM: char = '\u{F7FB}';
    pub const POINT_OF_SALE: char = '\u{F17E}';
    pub const POINT_SCAN: char = '\u{F70C}';
    pub const POKER_CHIP: char = '\u{F49B}';
    pub const POLICY: char = '\u{EA17}';
    pub const POLICY_ALERT: char = '\u{F407}';
    pub const POLL: char = '\u{F0CC}';
    pub const POLYLINE: char = '\u{EBBB}';
    pub const POLYMER: char = '\u{E8AB}';
    pub const POOL: char = '\u{EB48}';
    pub const PORTABLE_WIFI_OFF: char = '\u{F087}';
    pub const PORTRAIT: char = '\u{E851}';
    pub const POSITION_BOTTOM_LEFT: char = '\u{F70B}';
    pub const POSITION_BOTTOM_RIGHT: char = '\u{F70A}';
    pub const POSITION_TOP_RIGHT: char = '\u{F709}';
    pub const POST: char = '\u{E705}';
    pub const POST_ADD: char = '\u{EA20}';
    pub const POTTED_PLANT: char = '\u{F8AA}';
    pub const POWER: char = '\u{E63C}';
    pub const POWER_INPUT: char = '\u{E336}';
    pub const POWER_OFF: char = '\u{E646}';
    pub const POWER_ROUNDED: char = '\u{F8C7}';
    pub const POWER_SETTINGS_CIRCLE: char = '\u{F418}';
    pub const POWER_SETTINGS_NEW: char = '\u{F8C7}';
    pub const PRAYER_TIMES: char = '\u{F838}';
    pub const PRECISION_MANUFACTURING: char = '\u{F049}';
    pub const PREGNANCY: char = '\u{F5F1}';
    pub const PREGNANT_WOMAN: char = '\u{F5F1}';
    pub const PRELIMINARY: char = '\u{E7D8}';
    pub const PRESCRIPTIONS: char = '\u{E121}';
    pub const PRESENT_TO_ALL: char = '\u{E0DF}';
    pub const PREVIEW: char = '\u{F1C5}';
    pub const PREVIEW_OFF: char = '\u{F7AF}';
    pub const PRICE_CHANGE: char = '\u{F04A}';
    pub const PRICE_CHECK: char = '\u{F04B}';
    pub const PRINT: char = '\u{E8AD}';
    pub const PRINT_ADD: char = '\u{F7A2}';
    pub const PRINT_CONNECT: char = '\u{F7A1}';
    pub const PRINT_DISABLED: char = '\u{E9CF}';
    pub const PRINT_ERROR: char = '\u{F7A0}';
    pub const PRINT_LOCK: char = '\u{F651}';
    pub const PRIORITY: char = '\u{EFB4}';
    pub const PRIORITY_HIGH: char = '\u{E645}';
    pub const PRIVACY: char = '\u{F148}';
    pub const PRIVACY_TIP: char = '\u{F0DC}';
    pub const PRIVATE_CONNECTIVITY: char = '\u{E744}';
    pub const PROBLEM: char = '\u{E122}';
    pub const PROCEDURE: char = '\u{E651}';
    pub const PROCESS_CHART: char = '\u{F855}';
    pub const PRODUCTION_QUANTITY_LIMITS: char = '\u{E1D1}';
    pub const PRODUCTIVITY: char = '\u{E296}';
    pub const PROGRESS_ACTIVITY: char = '\u{E9D0}';
    pub const PROMPT_SUGGESTION: char = '\u{F4F6}';
    pub const PROPANE: char = '\u{EC14}';
    pub const PROPANE_TANK: char = '\u{EC13}';
    pub const PSYCHIATRY: char = '\u{E123}';
    pub const PSYCHOLOGY: char = '\u{EA4A}';
    pub const PSYCHOLOGY_ALT: char = '\u{F8EA}';
    pub const PUBLIC: char = '\u{E80B}';
    pub const PUBLIC_OFF: char = '\u{F1CA}';
    pub const PUBLISH: char = '\u{E255}';
    pub const PUBLISHED_WITH_CHANGES: char = '\u{F232}';
    pub const PULMONOLOGY: char = '\u{E124}';
    pub const PULSE_ALERT: char = '\u{F501}';
    pub const PUNCH_CLOCK: char = '\u{EAA8}';
    pub const PUSH_PIN: char = '\u{F10D}';
    pub const QR_CODE: char = '\u{EF6B}';
    pub const QR_CODE_2: char = '\u{E00A}';
    pub const QR_CODE_2_ADD: char = '\u{F658}';
    pub const QR_CODE_SCANNER: char = '\u{F206}';
    pub const QUERY_BUILDER: char = '\u{EFD6}';
    pub const QUERY_STATS: char = '\u{E4FC}';
    pub const QUESTION_ANSWER: char = '\u{E8AF}';
    pub const QUESTION_EXCHANGE: char = '\u{F7F3}';
    pub const QUESTION_MARK: char = '\u{EB8B}';
    pub const QUEUE: char = '\u{E03C}';
    pub const QUEUE_MUSIC: char = '\u{E03D}';
    pub const QUEUE_PLAY_NEXT: char = '\u{E066}';
    pub const QUICK_PHRASES: char = '\u{E7D1}';
    pub const QUICK_REFERENCE: char = '\u{E46E}';
    pub const QUICK_REFERENCE_ALL: char = '\u{F801}';
    pub const QUICK_REORDER: char = '\u{EB15}';
    pub const QUICKREPLY: char = '\u{EF6C}';
    pub const QUIET_TIME: char = '\u{F159}';
    pub const QUIET_TIME_ACTIVE: char = '\u{EB76}';
    pub const QUIZ: char = '\u{F04C}';
    pub const R_MOBILEDATA: char = '\u{F04D}';
    pub const RADAR: char = '\u{F04E}';
    pub const RADIO: char = '\u{E03E}';
    pub const RADIO_BUTTON_CHECKED: char = '\u{E837}';
    pub const RADIO_BUTTON_PARTIAL: char = '\u{F560}';
    pub const RADIO_BUTTON_UNCHECKED: char = '\u{E836}';
    pub const RADIOLOGY: char = '\u{E125}';
    pub const RAILWAY_ALERT: char = '\u{E9D1}';
    pub const RAILWAY_ALERT_2: char = '\u{F461}';
    pub const RAINY: char = '\u{F176}';
    pub const RAINY_HEAVY: char = '\u{F61F}';
    pub const RAINY_LIGHT: char = '\u{F61E}';
    pub const RAINY_SNOW: char = '\u{F61D}';
    pub const RAMEN_DINING: char = '\u{EA64}';
    pub const RAMP_LEFT: char = '\u{EB9C}';
    pub const RAMP_RIGHT: char = '\u{EB96}';
    pub const RANGE_HOOD: char = '\u{E1EA}';
    pub const RATE_REVIEW: char = '\u{E560}';
    pub const RATE_REVIEW_RTL: char = '\u{E706}';
    pub const RAVEN: char = '\u{F555}';
    pub const RAW_OFF: char = '\u{F04F}';
    pub const RAW_ON: char = '\u{F050}';
    pub const READ_MORE: char = '\u{EF6D}';
    pub const READINESS_SCORE: char = '\u{F6DD}';
    pub const REAL_ESTATE_AGENT: char = '\u{E73A}';
    pub const REAR_CAMERA: char = '\u{F6C2}';
    pub const REBASE: char = '\u{F845}';
    pub const REBASE_EDIT: char = '\u{F846}';
    pub const RECEIPT: char = '\u{E8B0}';
    pub const RECEIPT_LONG: char = '\u{EF6E}';
    pub const RECEIPT_LONG_OFF: char = '\u{F40A}';
    pub const RECENT_ACTORS: char = '\u{E03F}';
    pub const RECENT_PATIENT: char = '\u{F808}';
    pub const RECENTER: char = '\u{F4C0}';
    pub const RECOMMEND: char = '\u{E9D2}';
    pub const RECORD_VOICE_OVER: char = '\u{E91F}';
    pub const RECTANGLE: char = '\u{EB54}';
    pub const RECTANGLE_ADD: char = '\u{EEC8}';
    pub const RECYCLING: char = '\u{E760}';
    pub const REDEEM: char = '\u{E8F6}';
    pub const REDO: char = '\u{E15A}';
    pub const REDUCE_CAPACITY: char = '\u{F21C}';
    pub const REFRESH: char = '\u{E5D5}';
    pub const REGULAR_EXPRESSION: char = '\u{F750}';
    pub const RELAX: char = '\u{F6DC}';
    pub const RELEASE_ALERT: char = '\u{F654}';
    pub const REMEMBER_ME: char = '\u{F051}';
    pub const REMINDER: char = '\u{E6C6}';
    pub const REMINDERS_ALT: char = '\u{E6C6}';
    pub const REMOTE_GEN: char = '\u{E83E}';
    pub const REMOVE: char = '\u{E15B}';
    pub const REMOVE_CIRCLE: char = '\u{F08F}';
    pub const REMOVE_CIRCLE_OUTLINE: char = '\u{F08F}';
    pub const REMOVE_DONE: char = '\u{E9D3}';
    pub const REMOVE_FROM_QUEUE: char = '\u{E067}';
    pub const REMOVE_MODERATOR: char = '\u{E9D4}';
    pub const REMOVE_RED_EYE: char = '\u{E8F4}';
    pub const REMOVE_ROAD: char = '\u{EBFC}';
    pub const REMOVE_SELECTION: char = '\u{E9D5}';
    pub const REMOVE_SHOPPING_CART: char = '\u{E928}';
    pub const REOPEN_WINDOW: char = '\u{F708}';
    pub const REORDER: char = '\u{E8FE}';
    pub const REPARTITION: char = '\u{F8E8}';
    pub const REPEAT: char = '\u{E040}';
    pub const REPEAT_ON: char = '\u{E9D6}';
    pub const REPEAT_ONE: char = '\u{E041}';
    pub const REPEAT_ONE_ON: char = '\u{E9D7}';
    pub const REPLACE_AUDIO: char = '\u{F451}';
    pub const REPLACE_IMAGE: char = '\u{F450}';
    pub const REPLACE_VIDEO: char = '\u{F44F}';
    pub const REPLAY: char = '\u{E042}';
    pub const REPLAY_10: char = '\u{E059}';
    pub const REPLAY_30: char = '\u{E05A}';
    pub const REPLAY_5: char = '\u{E05B}';
    pub const REPLAY_CIRCLE_FILLED: char = '\u{E9D8}';
    pub const REPLY: char = '\u{E15E}';
    pub const REPLY_ALL: char = '\u{E15F}';
    pub const REPORT: char = '\u{F052}';
    pub const REPORT_GMAILERRORRED: char = '\u{F052}';
    pub const REPORT_OFF: char = '\u{E170}';
    pub const REPORT_PROBLEM: char = '\u{F083}';
    pub const REQUEST_PAGE: char = '\u{F22C}';
    pub const REQUEST_QUOTE: char = '\u{F1B6}';
    pub const RESET_BRIGHTNESS: char = '\u{F482}';
    pub const RESET_COLORS: char = '\u{FFEE2}';
    pub const RESET_EXPOSURE: char = '\u{F266}';
    pub const RESET_FOCUS: char = '\u{F481}';
    pub const RESET_IMAGE: char = '\u{F824}';
    pub const RESET_ISO: char = '\u{F480}';
    pub const RESET_SETTINGS: char = '\u{F47F}';
    pub const RESET_SHADOW: char = '\u{F47E}';
    pub const RESET_SHUTTER_SPEED: char = '\u{F47D}';
    pub const RESET_TV: char = '\u{E9D9}';
    pub const RESET_WHITE_BALANCE: char = '\u{F47C}';
    pub const RESET_WRENCH: char = '\u{F56C}';
    pub const RESIZE: char = '\u{F707}';
    pub const RESIZE_WINDOW: char = '\u{FFF99}';
    pub const RESPIRATORY_RATE: char = '\u{E127}';
    pub const RESPONSIVE_LAYOUT: char = '\u{E9DA}';
    pub const REST_AREA: char = '\u{F22A}';
    pub const RESTART_ALT: char = '\u{F053}';
    pub const RESTAURANT: char = '\u{E56C}';
    pub const RESTAURANT_MENU: char = '\u{E561}';
    pub const RESTORE: char = '\u{E8B3}';
    pub const RESTORE_FROM_TRASH: char = '\u{E938}';
    pub const RESTORE_PAGE: char = '\u{E929}';
    pub const RESUME: char = '\u{F7D0}';
    pub const REVIEWS: char = '\u{F07C}';
    pub const REWARDED_ADS: char = '\u{EFB6}';
    pub const RHEUMATOLOGY: char = '\u{E128}';
    pub const RIB_CAGE: char = '\u{F898}';
    pub const RICE_BOWL: char = '\u{F1F5}';
    pub const RIGHT_CLICK: char = '\u{F706}';
    pub const RIGHT_PANEL_CLOSE: char = '\u{F705}';
    pub const RIGHT_PANEL_OPEN: char = '\u{F704}';
    pub const RING_VOLUME: char = '\u{F0DD}';
    pub const RING_VOLUME_FILLED: char = '\u{F0DD}';
    pub const RIPPLES: char = '\u{E9DB}';
    pub const ROAD: char = '\u{F472}';
    pub const ROBOT: char = '\u{F882}';
    pub const ROBOT_2: char = '\u{F5D0}';
    pub const ROCKET: char = '\u{EBA5}';
    pub const ROCKET_LAUNCH: char = '\u{EB9B}';
    pub const ROLLER_SHADES: char = '\u{EC12}';
    pub const ROLLER_SHADES_CLOSED: char = '\u{EC11}';
    pub const ROLLER_SKATING: char = '\u{EBCD}';
    pub const ROOFING: char = '\u{F201}';
    pub const ROOM: char = '\u{F1DB}';
    pub const ROOM_PREFERENCES: char = '\u{F1B8}';
    pub const ROOM_SERVICE: char = '\u{EB49}';
    pub const ROTATE_90_DEGREES_CCW: char = '\u{E418}';
    pub const ROTATE_90_DEGREES_CW: char = '\u{EAAB}';
    pub const ROTATE_AUTO: char = '\u{F417}';
    pub const ROTATE_LEFT: char = '\u{E419}';
    pub const ROTATE_RIGHT: char = '\u{E41A}';
    pub const ROUNDABOUT_LEFT: char = '\u{EB99}';
    pub const ROUNDABOUT_RIGHT: char = '\u{EBA3}';
    pub const ROUNDED_CORNER: char = '\u{E920}';
    pub const ROUTE: char = '\u{EACD}';
    pub const ROUTER: char = '\u{E328}';
    pub const ROUTER_OFF: char = '\u{F2F4}';
    pub const ROUTINE: char = '\u{E20C}';
    pub const ROWING: char = '\u{E921}';
    pub const RSS_FEED: char = '\u{E0E5}';
    pub const RSVP: char = '\u{F055}';
    pub const RTT: char = '\u{E9AD}';
    pub const RUBRIC: char = '\u{EB27}';
    pub const RULE: char = '\u{F1C2}';
    pub const RULE_FOLDER: char = '\u{F1C9}';
    pub const RULE_SETTINGS: char = '\u{F64C}';
    pub const RUN_CIRCLE: char = '\u{EF6F}';
    pub const RUNNING_WITH_ERRORS: char = '\u{E51D}';
    pub const RV_HOOKUP: char = '\u{E642}';
    pub const SAFETY_CHECK: char = '\u{EBEF}';
    pub const SAFETY_CHECK_OFF: char = '\u{F59D}';
    pub const SAFETY_DIVIDER: char = '\u{E1CC}';
    pub const SAILING: char = '\u{E502}';
    pub const SALINITY: char = '\u{F876}';
    pub const SANITIZER: char = '\u{F21D}';
    pub const SATELLITE: char = '\u{E562}';
    pub const SATELLITE_ALT: char = '\u{EB3A}';
    pub const SAUNA: char = '\u{F6F7}';
    pub const SAVE: char = '\u{E161}';
    pub const SAVE_ALT: char = '\u{F090}';
    pub const SAVE_AS: char = '\u{EB60}';
    pub const SAVE_CLOCK: char = '\u{F398}';
    pub const SAVED_SEARCH: char = '\u{EA11}';
    pub const SAVINGS: char = '\u{E2EB}';
    pub const SCALE: char = '\u{EB5F}';
    pub const SCAN: char = '\u{F74E}';
    pub const SCAN_DELETE: char = '\u{F74F}';
    pub const SCANNER: char = '\u{E329}';
    pub const SCATTER_PLOT: char = '\u{E268}';
    pub const SCENE: char = '\u{E2A7}';
    pub const SCHEDULE: char = '\u{EFD6}';
    pub const SCHEDULE_SEND: char = '\u{EA0A}';
    pub const SCHEMA: char = '\u{E4FD}';
    pub const SCHOOL: char = '\u{E80C}';
    pub const SCIENCE: char = '\u{EA4B}';
    pub const SCIENCE_OFF: char = '\u{F542}';
    pub const SCOOTER: char = '\u{F471}';
    pub const SCORE: char = '\u{E269}';
    pub const SCOREBOARD: char = '\u{EBD0}';
    pub const SCREEN_LOCK_LANDSCAPE: char = '\u{F2D8}';
    pub const SCREEN_LOCK_PORTRAIT: char = '\u{F2BE}';
    pub const SCREEN_LOCK_ROTATION: char = '\u{F2D6}';
    pub const SCREEN_RECORD: char = '\u{F679}';
    pub const SCREEN_ROTATION: char = '\u{F2D5}';
    pub const SCREEN_ROTATION_ALT: char = '\u{EBEE}';
    pub const SCREEN_ROTATION_UP: char = '\u{F678}';
    pub const SCREEN_SEARCH_DESKTOP: char = '\u{EF70}';
    pub const SCREEN_SHARE: char = '\u{E0E2}';
    pub const SCREENSHOT: char = '\u{F056}';
    pub const SCREENSHOT_FRAME: char = '\u{F677}';
    pub const SCREENSHOT_FRAME_2: char = '\u{F374}';
    pub const SCREENSHOT_KEYBOARD: char = '\u{F7D3}';
    pub const SCREENSHOT_MONITOR: char = '\u{EC08}';
    pub const SCREENSHOT_REGION: char = '\u{F7D2}';
    pub const SCREENSHOT_TABLET: char = '\u{F697}';
    pub const SCRIPT: char = '\u{F45F}';
    pub const SCROLLABLE_HEADER: char = '\u{E9DC}';
    pub const SCUBA_DIVING: char = '\u{EBCE}';
    pub const SD: char = '\u{E9DD}';
    pub const SD_CARD: char = '\u{E623}';
    pub const SD_CARD_ALERT: char = '\u{F057}';
    pub const SD_STORAGE: char = '\u{E623}';
    pub const SDK: char = '\u{E720}';
    pub const SEARCH: char = '\u{EF7A}';
    pub const SEARCH_ACTIVITY: char = '\u{F3E5}';
    pub const SEARCH_CHECK: char = '\u{F800}';
    pub const SEARCH_CHECK_2: char = '\u{F469}';
    pub const SEARCH_GEAR: char = '\u{EEFA}';
    pub const SEARCH_HANDS_FREE: char = '\u{E696}';
    pub const SEARCH_INSIGHTS: char = '\u{F4BC}';
    pub const SEARCH_OFF: char = '\u{EA76}';
    pub const SEAT_COOL_LEFT: char = '\u{F331}';
    pub const SEAT_COOL_RIGHT: char = '\u{F330}';
    pub const SEAT_HEAT_LEFT: char = '\u{F32F}';
    pub const SEAT_HEAT_RIGHT: char = '\u{F32E}';
    pub const SEAT_READ: char = '\u{FFEDA}';
    pub const SEAT_VENT_LEFT: char = '\u{F32D}';
    pub const SEAT_VENT_RIGHT: char = '\u{F32C}';
    pub const SEAT_WINDOW: char = '\u{FFEE1}';
    pub const SECURITY: char = '\u{E32A}';
    pub const SECURITY_KEY: char = '\u{F503}';
    pub const SECURITY_UPDATE: char = '\u{F2CD}';
    pub const SECURITY_UPDATE_GOOD: char = '\u{F073}';
    pub const SECURITY_UPDATE_WARNING: char = '\u{F2D3}';
    pub const SEGMENT: char = '\u{E94B}';
    pub const SELECT: char = '\u{F74D}';
    pub const SELECT_ALL: char = '\u{E162}';
    pub const SELECT_CHECK_BOX: char = '\u{F1FE}';
    pub const SELECT_TO_SPEAK: char = '\u{F7CF}';
    pub const SELECT_WINDOW: char = '\u{E6FA}';
    pub const SELECT_WINDOW_2: char = '\u{F4C8}';
    pub const SELECT_WINDOW_OFF: char = '\u{E506}';
    pub const SELF_CARE: char = '\u{F86D}';
    pub const SELF_IMPROVEMENT: char = '\u{EA78}';
    pub const SELL: char = '\u{F05B}';
    pub const SELL_CLOUD: char = '\u{FFF7B}';
    pub const SEND: char = '\u{E163}';
    pub const SEND_AND_ARCHIVE: char = '\u{EA0C}';
    pub const SEND_MONEY: char = '\u{E8B7}';
    pub const SEND_TIME_EXTENSION: char = '\u{EADB}';
    pub const SEND_TO_MOBILE: char = '\u{F2D2}';
    pub const SENSOR_DOOR: char = '\u{F1B5}';
    pub const SENSOR_OCCUPIED: char = '\u{EC10}';
    pub const SENSOR_WINDOW: char = '\u{F1B4}';
    pub const SENSORS: char = '\u{E51E}';
    pub const SENSORS_KRX: char = '\u{F556}';
    pub const SENSORS_KRX_OFF: char = '\u{F515}';
    pub const SENSORS_OFF: char = '\u{E51F}';
    pub const SENTIMENT_CALM: char = '\u{F6A7}';
    pub const SENTIMENT_CONTENT: char = '\u{F6A6}';
    pub const SENTIMENT_DISSATISFIED: char = '\u{E811}';
    pub const SENTIMENT_EXCITED: char = '\u{F6A5}';
    pub const SENTIMENT_EXTREMELY_DISSATISFIED: char = '\u{F194}';
    pub const SENTIMENT_FRUSTRATED: char = '\u{F6A4}';
    pub const SENTIMENT_NEUTRAL: char = '\u{E812}';
    pub const SENTIMENT_SAD: char = '\u{F6A3}';
    pub const SENTIMENT_SATISFIED: char = '\u{E813}';
    pub const SENTIMENT_SATISFIED_ALT: char = '\u{E813}';
    pub const SENTIMENT_STRESSED: char = '\u{F6A2}';
    pub const SENTIMENT_VERY_DISSATISFIED: char = '\u{E814}';
    pub const SENTIMENT_VERY_SATISFIED: char = '\u{E815}';
    pub const SENTIMENT_WORRIED: char = '\u{F6A1}';
    pub const SERIF: char = '\u{F4AC}';
    pub const SERVER_PERSON: char = '\u{F3BD}';
    pub const SERVICE_TOOLBOX: char = '\u{E717}';
    pub const SET_MEAL: char = '\u{F1EA}';
    pub const SETTINGS: char = '\u{E8B8}';
    pub const SETTINGS_ACCESSIBILITY: char = '\u{F05D}';
    pub const SETTINGS_ACCOUNT_BOX: char = '\u{F835}';
    pub const SETTINGS_ALERT: char = '\u{F143}';
    pub const SETTINGS_APPLICATIONS: char = '\u{E8B9}';
    pub const SETTINGS_B_ROLL: char = '\u{F625}';
    pub const SETTINGS_BACKUP_RESTORE: char = '\u{E8BA}';
    pub const SETTINGS_BLUETOOTH: char = '\u{E8BB}';
    pub const SETTINGS_BRIGHTNESS: char = '\u{E8BD}';
    pub const SETTINGS_CELL: char = '\u{F2D1}';
    pub const SETTINGS_CINEMATIC_BLUR: char = '\u{F624}';
    pub const SETTINGS_ETHERNET: char = '\u{E8BE}';
    pub const SETTINGS_HEART: char = '\u{F522}';
    pub const SETTINGS_INPUT_ANTENNA: char = '\u{E8BF}';
    pub const SETTINGS_INPUT_COMPONENT: char = '\u{E8C1}';
    pub const SETTINGS_INPUT_COMPOSITE: char = '\u{E8C1}';
    pub const SETTINGS_INPUT_HDMI: char = '\u{E8C2}';
    pub const SETTINGS_INPUT_SVIDEO: char = '\u{E8C3}';
    pub const SETTINGS_MOTION_MODE: char = '\u{F833}';
    pub const SETTINGS_NIGHT_SIGHT: char = '\u{F832}';
    pub const SETTINGS_OVERSCAN: char = '\u{E8C4}';
    pub const SETTINGS_PANORAMA: char = '\u{F831}';
    pub const SETTINGS_PHONE: char = '\u{E8C5}';
    pub const SETTINGS_PHOTO_CAMERA: char = '\u{F834}';
    pub const SETTINGS_POWER: char = '\u{E8C6}';
    pub const SETTINGS_REMOTE: char = '\u{E8C7}';
    pub const SETTINGS_SCREEN: char = '\u{FFEDF}';
    pub const SETTINGS_SEATING: char = '\u{EF2D}';
    pub const SETTINGS_SLOW_MOTION: char = '\u{F623}';
    pub const SETTINGS_SUGGEST: char = '\u{F05E}';
    pub const SETTINGS_SYSTEM_DAYDREAM: char = '\u{E1C3}';
    pub const SETTINGS_TIMELAPSE: char = '\u{F622}';
    pub const SETTINGS_VIDEO_CAMERA: char = '\u{F621}';
    pub const SETTINGS_VOICE: char = '\u{E8C8}';
    pub const SETTOP_COMPONENT: char = '\u{E2AC}';
    pub const SEVERE_COLD: char = '\u{EBD3}';
    pub const SHADES: char = '\u{FFF73}';
    pub const SHADES_CLOSED: char = '\u{FFF74}';
    pub const SHADOW: char = '\u{E9DF}';
    pub const SHADOW_ADD: char = '\u{F584}';
    pub const SHADOW_MINUS: char = '\u{F583}';
    pub const SHAPE_LINE: char = '\u{F8D3}';
    pub const SHAPE_RECOGNITION: char = '\u{EB01}';
    pub const SHAPES: char = '\u{E602}';
    pub const SHARE: char = '\u{E80D}';
    pub const SHARE_ETA: char = '\u{E5F7}';
    pub const SHARE_LOCATION: char = '\u{F05F}';
    pub const SHARE_OFF: char = '\u{F6CB}';
    pub const SHARE_REVIEWS: char = '\u{F8A4}';
    pub const SHARE_WINDOWS: char = '\u{F613}';
    pub const SHAVED_ICE: char = '\u{F225}';
    pub const SHEETS_RTL: char = '\u{F823}';
    pub const SHELF_AUTO_HIDE: char = '\u{F703}';
    pub const SHELF_POSITION: char = '\u{F702}';
    pub const SHELVES: char = '\u{F86E}';
    pub const SHIELD: char = '\u{E9E0}';
    pub const SHIELD_CARD: char = '\u{FFF30}';
    pub const SHIELD_LOCK: char = '\u{F686}';
    pub const SHIELD_LOCKED: char = '\u{F592}';
    pub const SHIELD_MOON: char = '\u{EAA9}';
    pub const SHIELD_PERSON: char = '\u{F650}';
    pub const SHIELD_QUESTION: char = '\u{F529}';
    pub const SHIELD_RADAR: char = '\u{FFF2F}';
    pub const SHIELD_TOGGLE: char = '\u{F2AD}';
    pub const SHIELD_WATCH: char = '\u{F30F}';
    pub const SHIELD_WITH_HEART: char = '\u{E78F}';
    pub const SHIELD_WITH_HOUSE: char = '\u{E78D}';
    pub const SHIFT: char = '\u{E5F2}';
    pub const SHIFT_LOCK: char = '\u{F7AE}';
    pub const SHIFT_LOCK_OFF: char = '\u{F483}';
    pub const SHOE_CLEATS: char = '\u{FFFB3}';
    pub const SHOP: char = '\u{E8C9}';
    pub const SHOP_2: char = '\u{E8CA}';
    pub const SHOP_TWO: char = '\u{E8CA}';
    pub const SHOPPING_BAG: char = '\u{F1CC}';
    pub const SHOPPING_BAG_SPEED: char = '\u{F39A}';
    pub const SHOPPING_BASKET: char = '\u{E8CB}';
    pub const SHOPPING_CART: char = '\u{E8CC}';
    pub const SHOPPING_CART_CHECKOUT: char = '\u{EB88}';
    pub const SHOPPING_CART_OFF: char = '\u{F4F7}';
    pub const SHOPPINGMODE: char = '\u{EFB7}';
    pub const SHORT_STAY: char = '\u{E4D0}';
    pub const SHORT_TEXT: char = '\u{E261}';
    pub const SHORTCUT: char = '\u{F57A}';
    pub const SHOW_CHART: char = '\u{E6E1}';
    pub const SHOWER: char = '\u{F061}';
    pub const SHUFFLE: char = '\u{E043}';
    pub const SHUFFLE_ON: char = '\u{E9E1}';
    pub const SHUTTER_SPEED: char = '\u{E43D}';
    pub const SHUTTER_SPEED_ADD: char = '\u{F57E}';
    pub const SHUTTER_SPEED_MINUS: char = '\u{F57D}';
    pub const SICK: char = '\u{F220}';
    pub const SIDE_NAVIGATION: char = '\u{E9E2}';
    pub const SIGN_LANGUAGE: char = '\u{EBE5}';
    pub const SIGN_LANGUAGE_2: char = '\u{F258}';
    pub const SIGN_LANGUAGE_OFF: char = '\u{FFEE4}';
    pub const SIGNAL_CELLULAR_0_BAR: char = '\u{F0A8}';
    pub const SIGNAL_CELLULAR_1_BAR: char = '\u{F0A9}';
    pub const SIGNAL_CELLULAR_2_BAR: char = '\u{F0AA}';
    pub const SIGNAL_CELLULAR_3_BAR: char = '\u{F0AB}';
    pub const SIGNAL_CELLULAR_4_BAR: char = '\u{E1C8}';
    pub const SIGNAL_CELLULAR_ADD: char = '\u{F7A9}';
    pub const SIGNAL_CELLULAR_ALT: char = '\u{E202}';
    pub const SIGNAL_CELLULAR_ALT_1_BAR: char = '\u{EBDF}';
    pub const SIGNAL_CELLULAR_ALT_2_BAR: char = '\u{EBE3}';
    pub const SIGNAL_CELLULAR_ALT_OFF: char = '\u{FFF8A}';
    pub const SIGNAL_CELLULAR_CONNECTED_NO_INTERNET_0_BAR: char = '\u{F0AC}';
    pub const SIGNAL_CELLULAR_CONNECTED_NO_INTERNET_4_BAR: char = '\u{E1CD}';
    pub const SIGNAL_CELLULAR_NO_SIM: char = '\u{E1CE}';
    pub const SIGNAL_CELLULAR_NODATA: char = '\u{F062}';
    pub const SIGNAL_CELLULAR_NULL: char = '\u{E1CF}';
    pub const SIGNAL_CELLULAR_OFF: char = '\u{E1D0}';
    pub const SIGNAL_CELLULAR_PAUSE: char = '\u{F5A7}';
    pub const SIGNAL_DISCONNECTED: char = '\u{F239}';
    pub const SIGNAL_WIFI_0_BAR: char = '\u{F0B0}';
    pub const SIGNAL_WIFI_4_BAR: char = '\u{F065}';
    pub const SIGNAL_WIFI_4_BAR_LOCK: char = '\u{E1E1}';
    pub const SIGNAL_WIFI_BAD: char = '\u{F064}';
    pub const SIGNAL_WIFI_CONNECTED_NO_INTERNET_4: char = '\u{F064}';
    pub const SIGNAL_WIFI_OFF: char = '\u{E1DA}';
    pub const SIGNAL_WIFI_STATUSBAR_4_BAR: char = '\u{F065}';
    pub const SIGNAL_WIFI_STATUSBAR_NOT_CONNECTED: char = '\u{F0EF}';
    pub const SIGNAL_WIFI_STATUSBAR_NULL: char = '\u{F067}';
    pub const SIGNATURE: char = '\u{F74C}';
    pub const SIGNPOST: char = '\u{EB91}';
    pub const SIM_CARD: char = '\u{E32B}';
    pub const SIM_CARD_ALERT: char = '\u{F057}';
    pub const SIM_CARD_DOWNLOAD: char = '\u{F068}';
    pub const SIM_CARD_LOCK: char = '\u{FFEC1}';
    pub const SIMULATION: char = '\u{F3E1}';
    pub const SINGLE_ARROW: char = '\u{FFED9}';
    pub const SINGLE_BED: char = '\u{EA48}';
    pub const SIP: char = '\u{F069}';
    pub const SIREN: char = '\u{F3A7}';
    pub const SIREN_CHECK: char = '\u{F3A6}';
    pub const SIREN_OPEN: char = '\u{F3A5}';
    pub const SIREN_QUESTION: char = '\u{F3A4}';
    pub const SKATEBOARDING: char = '\u{E511}';
    pub const SKELETON: char = '\u{F899}';
    pub const SKILLET: char = '\u{F543}';
    pub const SKILLET_COOKTOP: char = '\u{F544}';
    pub const SKIP_NEXT: char = '\u{E044}';
    pub const SKIP_PREVIOUS: char = '\u{E045}';
    pub const SKULL: char = '\u{F89A}';
    pub const SKULL_LIST: char = '\u{F370}';
    pub const SLAB_SERIF: char = '\u{F4AB}';
    pub const SLEDDING: char = '\u{E512}';
    pub const SLEEP: char = '\u{E213}';
    pub const SLEEP_SCORE: char = '\u{F6B7}';
    pub const SLIDE_LIBRARY: char = '\u{F822}';
    pub const SLIDERS: char = '\u{E9E3}';
    pub const SLIDESHOW: char = '\u{E41B}';
    pub const SLOW_MOTION_VIDEO: char = '\u{E068}';
    pub const SMART_BUTTON: char = '\u{F1C1}';
    pub const SMART_CARD_READER: char = '\u{F4A5}';
    pub const SMART_CARD_READER_OFF: char = '\u{F4A6}';
    pub const SMART_DISPLAY: char = '\u{F06A}';
    pub const SMART_OUTLET: char = '\u{E844}';
    pub const SMART_SCREEN: char = '\u{F2D0}';
    pub const SMART_TOY: char = '\u{F06C}';
    pub const SMARTPHONE: char = '\u{E7BA}';
    pub const SMARTPHONE_CAMERA: char = '\u{F44E}';
    pub const SMB_SHARE: char = '\u{F74B}';
    pub const SMOKE_FREE: char = '\u{EB4A}';
    pub const SMOKING_ROOMS: char = '\u{EB4B}';
    pub const SMS: char = '\u{E625}';
    pub const SMS_FAILED: char = '\u{E87F}';
    pub const SNAIL: char = '\u{FFEDE}';
    pub const SNIPPET_FOLDER: char = '\u{F1C7}';
    pub const SNOOZE: char = '\u{E046}';
    pub const SNOWBOARDING: char = '\u{E513}';
    pub const SNOWFLAKE: char = '\u{ED5B}';
    pub const SNOWING: char = '\u{E80F}';
    pub const SNOWING_HEAVY: char = '\u{F61C}';
    pub const SNOWMOBILE: char = '\u{E503}';
    pub const SNOWSHOEING: char = '\u{E514}';
    pub const SOAP: char = '\u{F1B2}';
    pub const SOBA: char = '\u{EF36}';
    pub const SOCIAL_DISTANCE: char = '\u{E1CB}';
    pub const SOCIAL_LEADERBOARD: char = '\u{F6A0}';
    pub const SOLAR_POWER: char = '\u{EC0F}';
    pub const SOLO_DINING: char = '\u{EF35}';
    pub const SORT: char = '\u{E164}';
    pub const SORT_BY_ALPHA: char = '\u{E053}';
    pub const SOS: char = '\u{EBF7}';
    pub const SOUND_DETECTION_DOG_BARKING: char = '\u{F149}';
    pub const SOUND_DETECTION_GLASS_BREAK: char = '\u{F14A}';
    pub const SOUND_DETECTION_LOUD_SOUND: char = '\u{F14B}';
    pub const SOUND_SAMPLER: char = '\u{F6B4}';
    pub const SOUNDBAR: char = '\u{FFF72}';
    pub const SOUP_KITCHEN: char = '\u{E7D3}';
    pub const SOURCE: char = '\u{F1C8}';
    pub const SOURCE_ENVIRONMENT: char = '\u{E527}';
    pub const SOURCE_NOTES: char = '\u{E12D}';
    pub const SOUTH: char = '\u{F1E3}';
    pub const SOUTH_AMERICA: char = '\u{E7E4}';
    pub const SOUTH_EAST: char = '\u{F1E4}';
    pub const SOUTH_WEST: char = '\u{F1E5}';
    pub const SPA: char = '\u{EB4C}';
    pub const SPACE_BAR: char = '\u{E256}';
    pub const SPACE_DASHBOARD: char = '\u{E66B}';
    pub const SPACE_DASHBOARD_2: char = '\u{FFF8C}';
    pub const SPATIAL_AUDIO: char = '\u{EBEB}';
    pub const SPATIAL_AUDIO_OFF: char = '\u{EBE8}';
    pub const SPATIAL_GALLERY: char = '\u{FFEB6}';
    pub const SPATIAL_SPEAKER: char = '\u{F4CF}';
    pub const SPATIAL_TRACKING: char = '\u{EBEA}';
    pub const SPEAKER: char = '\u{E32D}';
    pub const SPEAKER_2: char = '\u{FFF71}';
    pub const SPEAKER_3: char = '\u{FFEB7}';
    pub const SPEAKER_GROUP: char = '\u{E32E}';
    pub const SPEAKER_NOTES: char = '\u{E8CD}';
    pub const SPEAKER_NOTES_OFF: char = '\u{E92A}';
    pub const SPEAKER_PHONE: char = '\u{E0D2}';
    pub const SPECIAL_CHARACTER: char = '\u{F74A}';
    pub const SPECIFIC_GRAVITY: char = '\u{F872}';
    pub const SPEECH_TO_TEXT: char = '\u{F8A7}';
    pub const SPEECH_TO_TEXT_2: char = '\u{FFEC9}';
    pub const SPEED: char = '\u{E9E4}';
    pub const SPEED_0_25: char = '\u{F4D4}';
    pub const SPEED_0_2X: char = '\u{F498}';
    pub const SPEED_0_5: char = '\u{F4E2}';
    pub const SPEED_0_5X: char = '\u{F497}';
    pub const SPEED_0_75: char = '\u{F4D3}';
    pub const SPEED_0_7X: char = '\u{F496}';
    pub const SPEED_1_2: char = '\u{F4E1}';
    pub const SPEED_1_25: char = '\u{F4D2}';
    pub const SPEED_1_2X: char = '\u{F495}';
    pub const SPEED_1_5: char = '\u{F4E0}';
    pub const SPEED_1_5X: char = '\u{F494}';
    pub const SPEED_1_75: char = '\u{F4D1}';
    pub const SPEED_1_7X: char = '\u{F493}';
    pub const SPEED_2: char = '\u{FFF38}';
    pub const SPEED_2X: char = '\u{F4EB}';
    pub const SPEED_3: char = '\u{FFF37}';
    pub const SPEED_4: char = '\u{FFF36}';
    pub const SPEED_CAMERA: char = '\u{F470}';
    pub const SPELLCHECK: char = '\u{E8CE}';
    pub const SPLIT_SCENE: char = '\u{F3BF}';
    pub const SPLIT_SCENE_2: char = '\u{FFEF7}';
    pub const SPLIT_SCENE_DOWN: char = '\u{F2FF}';
    pub const SPLIT_SCENE_LEFT: char = '\u{F2FE}';
    pub const SPLIT_SCENE_RIGHT: char = '\u{F2FD}';
    pub const SPLIT_SCENE_UP: char = '\u{F2FC}';
    pub const SPLITSCREEN: char = '\u{F06D}';
    pub const SPLITSCREEN_ADD: char = '\u{F4FD}';
    pub const SPLITSCREEN_BOTTOM: char = '\u{F676}';
    pub const SPLITSCREEN_LANDSCAPE: char = '\u{F459}';
    pub const SPLITSCREEN_LANDSCAPE_ADD: char = '\u{FFFBA}';
    pub const SPLITSCREEN_LEFT: char = '\u{F675}';
    pub const SPLITSCREEN_PORTRAIT: char = '\u{F458}';
    pub const SPLITSCREEN_RIGHT: char = '\u{F674}';
    pub const SPLITSCREEN_TOP: char = '\u{F673}';
    pub const SPLITSCREEN_VERTICAL_ADD: char = '\u{F4FC}';
    pub const SPO2: char = '\u{F6DB}';
    pub const SPOKE: char = '\u{E9A7}';
    pub const SPORTS: char = '\u{EA30}';
    pub const SPORTS_AND_OUTDOORS: char = '\u{EFB8}';
    pub const SPORTS_BAR: char = '\u{F1F3}';
    pub const SPORTS_BASEBALL: char = '\u{EA51}';
    pub const SPORTS_BASKETBALL: char = '\u{EA26}';
    pub const SPORTS_CRICKET: char = '\u{EA27}';
    pub const SPORTS_ESPORTS: char = '\u{EA28}';
    pub const SPORTS_FOOTBALL: char = '\u{EA29}';
    pub const SPORTS_GOLF: char = '\u{EA2A}';
    pub const SPORTS_GYMNASTICS: char = '\u{EBC4}';
    pub const SPORTS_HANDBALL: char = '\u{EA33}';
    pub const SPORTS_HOCKEY: char = '\u{EA2B}';
    pub const SPORTS_KABADDI: char = '\u{EA34}';
    pub const SPORTS_MARTIAL_ARTS: char = '\u{EAE9}';
    pub const SPORTS_MMA: char = '\u{EA2C}';
    pub const SPORTS_MOTORSPORTS: char = '\u{EA2D}';
    pub const SPORTS_RUGBY: char = '\u{EA2E}';
    pub const SPORTS_SCORE: char = '\u{F06E}';
    pub const SPORTS_SOCCER: char = '\u{EA2F}';
    pub const SPORTS_TENNIS: char = '\u{EA32}';
    pub const SPORTS_VOLLEYBALL: char = '\u{EA31}';
    pub const SPRINKLER: char = '\u{E29A}';
    pub const SPRINT: char = '\u{F81F}';
    pub const SQL: char = '\u{FFF95}';
    pub const SQUARE: char = '\u{EB36}';
    pub const SQUARE_CIRCLE: char = '\u{EEC7}';
    pub const SQUARE_DOT: char = '\u{F3B3}';
    pub const SQUARE_FOOT: char = '\u{EA49}';
    pub const SSID_CHART: char = '\u{EB66}';
    pub const STACK: char = '\u{F609}';
    pub const STACK_GROUP: char = '\u{F359}';
    pub const STACK_HEXAGON: char = '\u{F41C}';
    pub const STACK_OFF: char = '\u{F608}';
    pub const STACK_STAR: char = '\u{F607}';
    pub const STACKED_BAR_CHART: char = '\u{E9E6}';
    pub const STACKED_EMAIL: char = '\u{E6C7}';
    pub const STACKED_INBOX: char = '\u{E6C9}';
    pub const STACKED_LINE_CHART: char = '\u{F22B}';
    pub const STACKS: char = '\u{F500}';
    pub const STADIA_CONTROLLER: char = '\u{F135}';
    pub const STADIUM: char = '\u{EB90}';
    pub const STAIRS: char = '\u{F1A9}';
    pub const STAIRS_2: char = '\u{F46C}';
    pub const STAR: char = '\u{F09A}';
    pub const STAR_BORDER: char = '\u{F09A}';
    pub const STAR_BORDER_PURPLE500: char = '\u{F09A}';
    pub const STAR_HALF: char = '\u{E839}';
    pub const STAR_OUTLINE: char = '\u{F09A}';
    pub const STAR_PURPLE500: char = '\u{F09A}';
    pub const STAR_RATE: char = '\u{F0EC}';
    pub const STAR_RATE_HALF: char = '\u{EC45}';
    pub const STAR_SHINE: char = '\u{F31D}';
    pub const STARS: char = '\u{E8D0}';
    pub const STARS_2: char = '\u{F31C}';
    pub const START: char = '\u{E089}';
    pub const STAT_0: char = '\u{E697}';
    pub const STAT_1: char = '\u{E698}';
    pub const STAT_2: char = '\u{E699}';
    pub const STAT_3: char = '\u{E69A}';
    pub const STAT_MINUS_1: char = '\u{E69B}';
    pub const STAT_MINUS_2: char = '\u{E69C}';
    pub const STAT_MINUS_3: char = '\u{E69D}';
    pub const STAY_CURRENT_LANDSCAPE: char = '\u{ED3E}';
    pub const STAY_CURRENT_PORTRAIT: char = '\u{E7BA}';
    pub const STAY_PRIMARY_LANDSCAPE: char = '\u{ED3E}';
    pub const STAY_PRIMARY_PORTRAIT: char = '\u{F2D3}';
    pub const STEERING_WHEEL_COOL: char = '\u{FFEBD}';
    pub const STEERING_WHEEL_HEAT: char = '\u{F32B}';
    pub const STEP: char = '\u{F6FE}';
    pub const STEP_INTO: char = '\u{F701}';
    pub const STEP_OUT: char = '\u{F700}';
    pub const STEP_OVER: char = '\u{F6FF}';
    pub const STEPPERS: char = '\u{E9E7}';
    pub const STEPS: char = '\u{F6DA}';
    pub const STETHOSCOPE: char = '\u{F805}';
    pub const STETHOSCOPE_ARROW: char = '\u{F807}';
    pub const STETHOSCOPE_CHECK: char = '\u{F806}';
    pub const STICKER: char = '\u{E707}';
    pub const STICKER_ADD: char = '\u{EEC2}';
    pub const STICKY_NOTE: char = '\u{E9E8}';
    pub const STICKY_NOTE_2: char = '\u{F1FC}';
    pub const STOCK_MEDIA: char = '\u{F570}';
    pub const STOCKPOT: char = '\u{F545}';
    pub const STOP: char = '\u{E047}';
    pub const STOP_CIRCLE: char = '\u{EF71}';
    pub const STOP_SCREEN_SHARE: char = '\u{E0E3}';
    pub const STORAGE: char = '\u{E1DB}';
    pub const STORE: char = '\u{E8D1}';
    pub const STORE_MALL_DIRECTORY: char = '\u{E8D1}';
    pub const STOREFRONT: char = '\u{EA12}';
    pub const STORM: char = '\u{F070}';
    pub const STRAIGHT: char = '\u{EB95}';
    pub const STRAIGHTEN: char = '\u{E41C}';
    pub const STRATEGY: char = '\u{F5DF}';
    pub const STREAM: char = '\u{E9E9}';
    pub const STREAM_APPS: char = '\u{F79F}';
    pub const STREETVIEW: char = '\u{E56E}';
    pub const STRESS_MANAGEMENT: char = '\u{F6D9}';
    pub const STRIKETHROUGH_S: char = '\u{E257}';
    pub const STROKE_FULL: char = '\u{F749}';
    pub const STROKE_PARTIAL: char = '\u{F748}';
    pub const STROLLER: char = '\u{F1AE}';
    pub const STYLE: char = '\u{E41D}';
    pub const STYLER: char = '\u{E273}';
    pub const STYLUS: char = '\u{F604}';
    pub const STYLUS_BRUSH: char = '\u{F366}';
    pub const STYLUS_FOUNTAIN_PEN: char = '\u{F365}';
    pub const STYLUS_HIGHLIGHTER: char = '\u{F364}';
    pub const STYLUS_LASER_POINTER: char = '\u{F747}';
    pub const STYLUS_NOTE: char = '\u{F603}';
    pub const STYLUS_PEN: char = '\u{F363}';
    pub const STYLUS_PENCIL: char = '\u{F362}';
    pub const SUBDIRECTORY_ARROW_LEFT: char = '\u{E5D9}';
    pub const SUBDIRECTORY_ARROW_RIGHT: char = '\u{E5DA}';
    pub const SUBHEADER: char = '\u{E9EA}';
    pub const SUBJECT: char = '\u{E8D2}';
    pub const SUBSCRIPT: char = '\u{F111}';
    pub const SUBSCRIPTIONS: char = '\u{E064}';
    pub const SUBTITLES: char = '\u{E048}';
    pub const SUBTITLES_GEAR: char = '\u{F355}';
    pub const SUBTITLES_OFF: char = '\u{EF72}';
    pub const SUBWAY: char = '\u{E56F}';
    pub const SUBWAY_WALK: char = '\u{F287}';
    pub const SUBWOOFER: char = '\u{FFF70}';
    pub const SUMMARIZE: char = '\u{F071}';
    pub const SUNNY: char = '\u{E81A}';
    pub const SUNNY_SNOWING: char = '\u{E819}';
    pub const SUPERSCRIPT: char = '\u{F112}';
    pub const SUPERVISED_USER_CIRCLE: char = '\u{E939}';
    pub const SUPERVISED_USER_CIRCLE_OFF: char = '\u{F60E}';
    pub const SUPERVISOR_ACCOUNT: char = '\u{E8D3}';
    pub const SUPPORT: char = '\u{EF73}';
    pub const SUPPORT_AGENT: char = '\u{F0E2}';
    pub const SURFING: char = '\u{E515}';
    pub const SURGICAL: char = '\u{E131}';
    pub const SURROUND_SOUND: char = '\u{E049}';
    pub const SWAP_CALLS: char = '\u{E0D7}';
    pub const SWAP_DRIVING_APPS: char = '\u{E69E}';
    pub const SWAP_DRIVING_APPS_WHEEL: char = '\u{E69F}';
    pub const SWAP_HORIZ: char = '\u{E8D4}';
    pub const SWAP_HORIZONTAL_CIRCLE: char = '\u{E933}';
    pub const SWAP_VERT: char = '\u{E8D5}';
    pub const SWAP_VERTICAL_CIRCLE: char = '\u{E8D6}';
    pub const SWEEP: char = '\u{E6AC}';
    pub const SWIPE: char = '\u{E9EC}';
    pub const SWIPE_DOWN: char = '\u{EB53}';
    pub const SWIPE_DOWN_ALT: char = '\u{EB30}';
    pub const SWIPE_LEFT: char = '\u{EB59}';
    pub const SWIPE_LEFT_2: char = '\u{FFF94}';
    pub const SWIPE_LEFT_ALT: char = '\u{EB33}';
    pub const SWIPE_RIGHT: char = '\u{EB52}';
    pub const SWIPE_RIGHT_2: char = '\u{FFF93}';
    pub const SWIPE_RIGHT_ALT: char = '\u{EB56}';
    pub const SWIPE_UP: char = '\u{EB2E}';
    pub const SWIPE_UP_ALT: char = '\u{EB35}';
    pub const SWIPE_VERTICAL: char = '\u{EB51}';
    pub const SWITCH: char = '\u{E1F4}';
    pub const SWITCH_ACCESS: char = '\u{F6FD}';
    pub const SWITCH_ACCESS_2: char = '\u{F506}';
    pub const SWITCH_ACCESS_3: char = '\u{F34D}';
    pub const SWITCH_ACCESS_SHORTCUT: char = '\u{E7E1}';
    pub const SWITCH_ACCESS_SHORTCUT_ADD: char = '\u{E7E2}';
    pub const SWITCH_ACCOUNT: char = '\u{E9ED}';
    pub const SWITCH_CAMERA: char = '\u{E41E}';
    pub const SWITCH_LEFT: char = '\u{F1D1}';
    pub const SWITCH_OFF: char = '\u{FFF6F}';
    pub const SWITCH_RIGHT: char = '\u{F1D2}';
    pub const SWITCH_VIDEO: char = '\u{E41F}';
    pub const SWITCHES: char = '\u{E733}';
    pub const SWORD_ROSE: char = '\u{F5DE}';
    pub const SWORDS: char = '\u{F889}';
    pub const SYMPTOMS: char = '\u{E132}';
    pub const SYNAGOGUE: char = '\u{EAB0}';
    pub const SYNC: char = '\u{E627}';
    pub const SYNC_ALT: char = '\u{EA18}';
    pub const SYNC_ARROW_DOWN: char = '\u{F37C}';
    pub const SYNC_ARROW_UP: char = '\u{F37B}';
    pub const SYNC_DESKTOP: char = '\u{F41A}';
    pub const SYNC_DISABLED: char = '\u{E628}';
    pub const SYNC_LOCK: char = '\u{EAEE}';
    pub const SYNC_PROBLEM: char = '\u{E629}';
    pub const SYNC_SAVED_LOCALLY: char = '\u{F820}';
    pub const SYNC_SAVED_LOCALLY_OFF: char = '\u{F264}';
    pub const SYRINGE: char = '\u{E133}';
    pub const SYSTEM_SECURITY_UPDATE: char = '\u{F2CD}';
    pub const SYSTEM_SECURITY_UPDATE_GOOD: char = '\u{F073}';
    pub const SYSTEM_SECURITY_UPDATE_WARNING: char = '\u{F2D3}';
    pub const SYSTEM_UPDATE: char = '\u{F2CD}';
    pub const SYSTEM_UPDATE_ALT: char = '\u{E8D7}';
    pub const TAB: char = '\u{E8D8}';
    pub const TAB_CLOSE: char = '\u{F745}';
    pub const TAB_CLOSE_INACTIVE: char = '\u{F3D0}';
    pub const TAB_CLOSE_RIGHT: char = '\u{F746}';
    pub const TAB_DUPLICATE: char = '\u{F744}';
    pub const TAB_GROUP: char = '\u{F743}';
    pub const TAB_INACTIVE: char = '\u{F43B}';
    pub const TAB_MOVE: char = '\u{F742}';
    pub const TAB_NEW_RIGHT: char = '\u{F741}';
    pub const TAB_RECENT: char = '\u{F740}';
    pub const TAB_SEARCH: char = '\u{F2F2}';
    pub const TAB_UNSELECTED: char = '\u{E8D9}';
    pub const TABLE: char = '\u{F191}';
    pub const TABLE_BAR: char = '\u{EAD2}';
    pub const TABLE_CHART: char = '\u{E265}';
    pub const TABLE_CHART_VIEW: char = '\u{F6EF}';
    pub const TABLE_CONVERT: char = '\u{F3C7}';
    pub const TABLE_EDIT: char = '\u{F3C6}';
    pub const TABLE_EYE: char = '\u{F466}';
    pub const TABLE_LAMP: char = '\u{E1F2}';
    pub const TABLE_LARGE: char = '\u{F299}';
    pub const TABLE_RESTAURANT: char = '\u{EAC6}';
    pub const TABLE_ROWS: char = '\u{F101}';
    pub const TABLE_ROWS_NARROW: char = '\u{F73F}';
    pub const TABLE_SIGN: char = '\u{EF2C}';
    pub const TABLE_VIEW: char = '\u{F1BE}';
    pub const TABLET: char = '\u{E32F}';
    pub const TABLET_ANDROID: char = '\u{E330}';
    pub const TABLET_CAMERA: char = '\u{F44D}';
    pub const TABLET_MAC: char = '\u{E331}';
    pub const TABS: char = '\u{E9EE}';
    pub const TACTIC: char = '\u{F564}';
    pub const TAG: char = '\u{E9EF}';
    pub const TAG_FACES: char = '\u{EA22}';
    pub const TAKEOUT_DINING: char = '\u{EA74}';
    pub const TAKEOUT_DINING_2: char = '\u{EF34}';
    pub const TAMPER_DETECTION_OFF: char = '\u{E82E}';
    pub const TAMPER_DETECTION_ON: char = '\u{F8C8}';
    pub const TAP_AND_PLAY: char = '\u{F2CC}';
    pub const TAPAS: char = '\u{F1E9}';
    pub const TARGET: char = '\u{E719}';
    pub const TARGET_CHECK: char = '\u{FFEB3}';
    pub const TASK: char = '\u{F075}';
    pub const TASK_ALT: char = '\u{E2E6}';
    pub const TATAMI_SEAT: char = '\u{EF33}';
    pub const TAUNT: char = '\u{F69F}';
    pub const TAXI_ALERT: char = '\u{EF74}';
    pub const TEAM_DASHBOARD: char = '\u{E013}';
    pub const TEMP_PREFERENCES_CUSTOM: char = '\u{F8C9}';
    pub const TEMP_PREFERENCES_ECO: char = '\u{F8CA}';
    pub const TEMPLE_BUDDHIST: char = '\u{EAB3}';
    pub const TEMPLE_HINDU: char = '\u{EAAF}';
    pub const TENANCY: char = '\u{F0E3}';
    pub const TERMINAL: char = '\u{EB8E}';
    pub const TERMINAL_2: char = '\u{FFF8E}';
    pub const TERMINAL_ADD: char = '\u{FFED3}';
    pub const TERRAIN: char = '\u{E564}';
    pub const TEXT_AD: char = '\u{E728}';
    pub const TEXT_AD_OFF: char = '\u{FFF92}';
    pub const TEXT_COMPARE: char = '\u{F3C5}';
    pub const TEXT_DECREASE: char = '\u{EADD}';
    pub const TEXT_FIELDS: char = '\u{E262}';
    pub const TEXT_FIELDS_ALT: char = '\u{E9F1}';
    pub const TEXT_FORMAT: char = '\u{E165}';
    pub const TEXT_INCREASE: char = '\u{EAE2}';
    pub const TEXT_ROTATE_UP: char = '\u{E93A}';
    pub const TEXT_ROTATE_VERTICAL: char = '\u{E93B}';
    pub const TEXT_ROTATION_ANGLEDOWN: char = '\u{E93C}';
    pub const TEXT_ROTATION_ANGLEUP: char = '\u{E93D}';
    pub const TEXT_ROTATION_DOWN: char = '\u{E93E}';
    pub const TEXT_ROTATION_NONE: char = '\u{E93F}';
    pub const TEXT_SELECT_END: char = '\u{F73E}';
    pub const TEXT_SELECT_JUMP_TO_BEGINNING: char = '\u{F73D}';
    pub const TEXT_SELECT_JUMP_TO_END: char = '\u{F73C}';
    pub const TEXT_SELECT_MOVE_BACK_CHARACTER: char = '\u{F73B}';
    pub const TEXT_SELECT_MOVE_BACK_WORD: char = '\u{F73A}';
    pub const TEXT_SELECT_MOVE_DOWN: char = '\u{F739}';
    pub const TEXT_SELECT_MOVE_FORWARD_CHARACTER: char = '\u{F738}';
    pub const TEXT_SELECT_MOVE_FORWARD_WORD: char = '\u{F737}';
    pub const TEXT_SELECT_MOVE_UP: char = '\u{F736}';
    pub const TEXT_SELECT_START: char = '\u{F735}';
    pub const TEXT_SNIPPET: char = '\u{F1C6}';
    pub const TEXT_TO_SPEECH: char = '\u{F1BC}';
    pub const TEXT_UP: char = '\u{F49E}';
    pub const TEXTSMS: char = '\u{E625}';
    pub const TEXTURE: char = '\u{E421}';
    pub const TEXTURE_ADD: char = '\u{F57C}';
    pub const TEXTURE_MINUS: char = '\u{F57B}';
    pub const THEATER_COMEDY: char = '\u{EA66}';
    pub const THEATERS: char = '\u{E8DA}';
    pub const THERMOMETER: char = '\u{E846}';
    pub const THERMOMETER_ADD: char = '\u{F582}';
    pub const THERMOMETER_ALERT: char = '\u{FFFFB}';
    pub const THERMOMETER_GAIN: char = '\u{F6D8}';
    pub const THERMOMETER_LOSS: char = '\u{F6D7}';
    pub const THERMOMETER_MINUS: char = '\u{F581}';
    pub const THERMOSTAT: char = '\u{F076}';
    pub const THERMOSTAT_ARROW_DOWN: char = '\u{F37A}';
    pub const THERMOSTAT_ARROW_UP: char = '\u{F379}';
    pub const THERMOSTAT_AUTO: char = '\u{F077}';
    pub const THERMOSTAT_CARBON: char = '\u{F178}';
    pub const THINGS_TO_DO: char = '\u{EB2A}';
    pub const THREAD_UNREAD: char = '\u{F4F9}';
    pub const THREAT_INTELLIGENCE: char = '\u{EAED}';
    pub const THUMB_DOWN: char = '\u{F578}';
    pub const THUMB_DOWN_ALT: char = '\u{F578}';
    pub const THUMB_DOWN_FILLED: char = '\u{F578}';
    pub const THUMB_DOWN_OFF: char = '\u{F578}';
    pub const THUMB_DOWN_OFF_ALT: char = '\u{F578}';
    pub const THUMB_UP: char = '\u{F577}';
    pub const THUMB_UP_ALT: char = '\u{F577}';
    pub const THUMB_UP_FILLED: char = '\u{F577}';
    pub const THUMB_UP_OFF: char = '\u{F577}';
    pub const THUMB_UP_OFF_ALT: char = '\u{F577}';
    pub const THUMBNAIL_BAR: char = '\u{F734}';
    pub const THUMBS_UP_DOUBLE: char = '\u{EEFC}';
    pub const THUMBS_UP_DOWN: char = '\u{E8DD}';
    pub const THUNDERSTORM: char = '\u{EBDB}';
    pub const TIBIA: char = '\u{F89B}';
    pub const TIBIA_ALT: char = '\u{F89C}';
    pub const TILE_LARGE: char = '\u{F3C3}';
    pub const TILE_MEDIUM: char = '\u{F3C2}';
    pub const TILE_SMALL: char = '\u{F3C1}';
    pub const TILT_ARROW_DOWN: char = '\u{FFF26}';
    pub const TILT_ARROW_UP: char = '\u{FFF25}';
    pub const TIME_AUTO: char = '\u{F0E4}';
    pub const TIME_TO_LEAVE: char = '\u{EFF7}';
    pub const TIMELAPSE: char = '\u{E422}';
    pub const TIMELINE: char = '\u{E922}';
    pub const TIMER: char = '\u{E425}';
    pub const TIMER_1: char = '\u{F2AF}';
    pub const TIMER_10: char = '\u{E423}';
    pub const TIMER_10_ALT_1: char = '\u{EFBF}';
    pub const TIMER_10_SELECT: char = '\u{F07A}';
    pub const TIMER_2: char = '\u{F2AE}';
    pub const TIMER_3: char = '\u{E424}';
    pub const TIMER_3_ALT_1: char = '\u{EFC0}';
    pub const TIMER_3_SELECT: char = '\u{F07B}';
    pub const TIMER_5: char = '\u{F4B1}';
    pub const TIMER_5_SHUTTER: char = '\u{F4B2}';
    pub const TIMER_ARROW_DOWN: char = '\u{F378}';
    pub const TIMER_ARROW_UP: char = '\u{F377}';
    pub const TIMER_OFF: char = '\u{E426}';
    pub const TIMER_PAUSE: char = '\u{F4BB}';
    pub const TIMER_PLAY: char = '\u{F4BA}';
    pub const TIPS_AND_UPDATES: char = '\u{E79A}';
    pub const TIRE_REPAIR: char = '\u{EBC8}';
    pub const TITLE: char = '\u{E264}';
    pub const TITLECASE: char = '\u{F489}';
    pub const TOAST: char = '\u{EFC1}';
    pub const TOC: char = '\u{E8DE}';
    pub const TODAY: char = '\u{E8DF}';
    pub const TOGGLE_OFF: char = '\u{E9F5}';
    pub const TOGGLE_ON: char = '\u{E9F6}';
    pub const TOKEN: char = '\u{EA25}';
    pub const TOLL: char = '\u{E8E0}';
    pub const TONALITY: char = '\u{E427}';
    pub const TONALITY_2: char = '\u{F2B4}';
    pub const TOOLBAR: char = '\u{E9F7}';
    pub const TOOLS_FLAT_HEAD: char = '\u{F8CB}';
    pub const TOOLS_INSTALLATION_KIT: char = '\u{E2AB}';
    pub const TOOLS_LADDER: char = '\u{E2CB}';
    pub const TOOLS_LEVEL: char = '\u{E77B}';
    pub const TOOLS_PHILLIPS: char = '\u{F8CC}';
    pub const TOOLS_PLIERS_WIRE_STRIPPER: char = '\u{E2AA}';
    pub const TOOLS_POWER_DRILL: char = '\u{E1E9}';
    pub const TOOLS_WRENCH: char = '\u{F8CD}';
    pub const TOOLTIP: char = '\u{E9F8}';
    pub const TOOLTIP_2: char = '\u{F3ED}';
    pub const TOP_PANEL_CLOSE: char = '\u{F733}';
    pub const TOP_PANEL_OPEN: char = '\u{F732}';
    pub const TOPIC: char = '\u{F1C8}';
    pub const TORNADO: char = '\u{E199}';
    pub const TOTAL_DISSOLVED_SOLIDS: char = '\u{F877}';
    pub const TOUCH_APP: char = '\u{E913}';
    pub const TOUCH_DOUBLE: char = '\u{F38B}';
    pub const TOUCH_DOUBLE_2: char = '\u{FFF35}';
    pub const TOUCH_LONG: char = '\u{F38A}';
    pub const TOUCH_TRIPLE: char = '\u{F389}';
    pub const TOUCHPAD_MOUSE: char = '\u{F687}';
    pub const TOUCHPAD_MOUSE_OFF: char = '\u{F4E6}';
    pub const TOUR: char = '\u{EF75}';
    pub const TOYS: char = '\u{E332}';
    pub const TOYS_AND_GAMES: char = '\u{EFC2}';
    pub const TOYS_FAN: char = '\u{F887}';
    pub const TRACK_CHANGES: char = '\u{E8E1}';
    pub const TRACKPAD_INPUT: char = '\u{F4C7}';
    pub const TRACKPAD_INPUT_2: char = '\u{F409}';
    pub const TRACKPAD_INPUT_3: char = '\u{F408}';
    pub const TRAFFIC: char = '\u{E565}';
    pub const TRAFFIC_JAM: char = '\u{F46F}';
    pub const TRAIL_LENGTH: char = '\u{EB5E}';
    pub const TRAIL_LENGTH_MEDIUM: char = '\u{EB63}';
    pub const TRAIL_LENGTH_SHORT: char = '\u{EB6D}';
    pub const TRAIN: char = '\u{E570}';
    pub const TRAM: char = '\u{E571}';
    pub const TRANSCRIBE: char = '\u{F8EC}';
    pub const TRANSFER_WITHIN_A_STATION: char = '\u{E572}';
    pub const TRANSFORM: char = '\u{E428}';
    pub const TRANSGENDER: char = '\u{E58D}';
    pub const TRANSIT_ENTEREXIT: char = '\u{E579}';
    pub const TRANSIT_TICKET: char = '\u{F3F1}';
    pub const TRANSITION_CHOP: char = '\u{F50E}';
    pub const TRANSITION_DISSOLVE: char = '\u{F50D}';
    pub const TRANSITION_FADE: char = '\u{F50C}';
    pub const TRANSITION_PUSH: char = '\u{F50B}';
    pub const TRANSITION_SLIDE: char = '\u{F50A}';
    pub const TRANSLATE: char = '\u{E8E2}';
    pub const TRANSLATE_INDIC: char = '\u{F263}';
    pub const TRANSPORTATION: char = '\u{E21D}';
    pub const TRAVEL: char = '\u{EF93}';
    pub const TRAVEL_EXPLORE: char = '\u{E2DB}';
    pub const TRAVEL_LUGGAGE_AND_BAGS: char = '\u{EFC3}';
    pub const TRENDING_DOWN: char = '\u{E8E3}';
    pub const TRENDING_FLAT: char = '\u{E8E4}';
    pub const TRENDING_UP: char = '\u{E8E5}';
    pub const TRIANGLE_CIRCLE: char = '\u{EEC6}';
    pub const TRIP: char = '\u{E6FB}';
    pub const TRIP_ORIGIN: char = '\u{E57B}';
    pub const TROLLEY: char = '\u{F86B}';
    pub const TROLLEY_CABLE_CAR: char = '\u{F46E}';
    pub const TROPHY: char = '\u{EA23}';
    pub const TROUBLESHOOT: char = '\u{E1D2}';
    pub const TRY: char = '\u{F07C}';
    pub const TSUNAMI: char = '\u{EBD8}';
    pub const TSV: char = '\u{E6D6}';
    pub const TTY: char = '\u{F1AA}';
    pub const TUNE: char = '\u{E429}';
    pub const TUNGSTEN: char = '\u{F07D}';
    pub const TURN_LEFT: char = '\u{EBA6}';
    pub const TURN_RIGHT: char = '\u{EBAB}';
    pub const TURN_SHARP_LEFT: char = '\u{EBA7}';
    pub const TURN_SHARP_RIGHT: char = '\u{EBAA}';
    pub const TURN_SLIGHT_LEFT: char = '\u{EBA4}';
    pub const TURN_SLIGHT_RIGHT: char = '\u{EB9A}';
    pub const TURNED_IN: char = '\u{E8E7}';
    pub const TURNED_IN_NOT: char = '\u{E8E7}';
    pub const TV: char = '\u{E63B}';
    pub const TV_DISPLAYS: char = '\u{F3EC}';
    pub const TV_GEN: char = '\u{E830}';
    pub const TV_GUIDE: char = '\u{E1DC}';
    pub const TV_NEXT: char = '\u{F3EB}';
    pub const TV_OFF: char = '\u{E647}';
    pub const TV_OPTIONS_EDIT_CHANNELS: char = '\u{E1DD}';
    pub const TV_OPTIONS_INPUT_SETTINGS: char = '\u{E1DE}';
    pub const TV_REMOTE: char = '\u{F5D9}';
    pub const TV_SIGNIN: char = '\u{E71B}';
    pub const TV_WITH_ASSISTANT: char = '\u{E785}';
    pub const TWO_PAGER: char = '\u{F51F}';
    pub const TWO_PAGER_STORE: char = '\u{F3C4}';
    pub const TWO_WHEELER: char = '\u{E9F9}';
    pub const TYPE_SPECIMEN: char = '\u{F8F0}';
    pub const U_TURN_LEFT: char = '\u{EBA1}';
    pub const U_TURN_RIGHT: char = '\u{EBA2}';
    pub const UDON: char = '\u{EF32}';
    pub const ULNA_RADIUS: char = '\u{F89D}';
    pub const ULNA_RADIUS_ALT: char = '\u{F89E}';
    pub const UMBRELLA: char = '\u{F1AD}';
    pub const UNARCHIVE: char = '\u{E169}';
    pub const UNDEREYE: char = '\u{EEB1}';
    pub const UNDO: char = '\u{E166}';
    pub const UNFOLD_LESS: char = '\u{E5D6}';
    pub const UNFOLD_LESS_DOUBLE: char = '\u{F8CF}';
    pub const UNFOLD_MORE: char = '\u{E5D7}';
    pub const UNFOLD_MORE_DOUBLE: char = '\u{F8D0}';
    pub const UNGROUP: char = '\u{F731}';
    pub const UNIVERSAL_CURRENCY: char = '\u{E9FA}';
    pub const UNIVERSAL_CURRENCY_ALT: char = '\u{E734}';
    pub const UNIVERSAL_LOCAL: char = '\u{E9FB}';
    pub const UNKNOWN_2: char = '\u{F49F}';
    pub const UNKNOWN_5: char = '\u{E6A5}';
    pub const UNKNOWN_7: char = '\u{F49E}';
    pub const UNKNOWN_DOCUMENT: char = '\u{F804}';
    pub const UNKNOWN_MED: char = '\u{EABD}';
    pub const UNLICENSE: char = '\u{EB05}';
    pub const UNPAVED_ROAD: char = '\u{F46D}';
    pub const UNPIN: char = '\u{E6F9}';
    pub const UNPUBLISHED: char = '\u{F236}';
    pub const UNSUBSCRIBE: char = '\u{E0EB}';
    pub const UPCOMING: char = '\u{F07E}';
    pub const UPDATE: char = '\u{E923}';
    pub const UPDATE_DISABLED: char = '\u{E075}';
    pub const UPGRADE: char = '\u{F0FB}';
    pub const UPI_PAY: char = '\u{F3CF}';
    pub const UPLOAD: char = '\u{F09B}';
    pub const UPLOAD_2: char = '\u{F521}';
    pub const UPLOAD_FILE: char = '\u{E9FC}';
    pub const UPPERCASE: char = '\u{F488}';
    pub const UROLOGY: char = '\u{E137}';
    pub const USB: char = '\u{E1E0}';
    pub const USB_OFF: char = '\u{E4FA}';
    pub const USER_ATTRIBUTES: char = '\u{E708}';
    pub const VACCINES: char = '\u{E138}';
    pub const VACUUM: char = '\u{EFC5}';
    pub const VACUUM_2: char = '\u{FFF6D}';
    pub const VACUUM_2_ON: char = '\u{FFF6E}';
    pub const VALVE: char = '\u{E224}';
    pub const VAPE_FREE: char = '\u{EBC6}';
    pub const VAPING_ROOMS: char = '\u{EBCF}';
    pub const VARIABLE_ADD: char = '\u{F51E}';
    pub const VARIABLE_INSERT: char = '\u{F51D}';
    pub const VARIABLE_REMOVE: char = '\u{F51C}';
    pub const VARIABLES: char = '\u{F851}';
    pub const VENTILATOR: char = '\u{E139}';
    pub const VERIFIED: char = '\u{EF76}';
    pub const VERIFIED_OFF: char = '\u{F30E}';
    pub const VERIFIED_USER: char = '\u{F013}';
    pub const VERTICAL_ALIGN_BOTTOM: char = '\u{E258}';
    pub const VERTICAL_ALIGN_CENTER: char = '\u{E259}';
    pub const VERTICAL_ALIGN_TOP: char = '\u{E25A}';
    pub const VERTICAL_DISTRIBUTE: char = '\u{E076}';
    pub const VERTICAL_SHADES: char = '\u{EC0E}';
    pub const VERTICAL_SHADES_CLOSED: char = '\u{EC0D}';
    pub const VERTICAL_SPLIT: char = '\u{E949}';
    pub const VIBRATION: char = '\u{F2CB}';
    pub const VIDEO_CALL: char = '\u{E070}';
    pub const VIDEO_CAMERA_BACK: char = '\u{F07F}';
    pub const VIDEO_CAMERA_BACK_ADD: char = '\u{F40C}';
    pub const VIDEO_CAMERA_FRONT: char = '\u{F080}';
    pub const VIDEO_CAMERA_FRONT_OFF: char = '\u{F83B}';
    pub const VIDEO_CHAT: char = '\u{F8A0}';
    pub const VIDEO_FILE: char = '\u{EB87}';
    pub const VIDEO_FRAME_COPY: char = '\u{FFF0D}';
    pub const VIDEO_FRAME_SAVE: char = '\u{FFF0C}';
    pub const VIDEO_LABEL: char = '\u{E071}';
    pub const VIDEO_LIBRARY: char = '\u{E04A}';
    pub const VIDEO_SEARCH: char = '\u{EFC6}';
    pub const VIDEO_SETTINGS: char = '\u{EA75}';
    pub const VIDEO_STABLE: char = '\u{F081}';
    pub const VIDEO_TEMPLATE: char = '\u{FFFD3}';
    pub const VIDEOCAM: char = '\u{E04B}';
    pub const VIDEOCAM_ALERT: char = '\u{F390}';
    pub const VIDEOCAM_OFF: char = '\u{E04C}';
    pub const VIDEOGAME_ASSET: char = '\u{E338}';
    pub const VIDEOGAME_ASSET_OFF: char = '\u{E500}';
    pub const VIEW_AGENDA: char = '\u{E8E9}';
    pub const VIEW_APPS: char = '\u{F376}';
    pub const VIEW_ARRAY: char = '\u{E8EA}';
    pub const VIEW_CAROUSEL: char = '\u{E8EB}';
    pub const VIEW_COLUMN: char = '\u{E8EC}';
    pub const VIEW_COLUMN_2: char = '\u{F847}';
    pub const VIEW_COMFY: char = '\u{E42A}';
    pub const VIEW_COMFY_ALT: char = '\u{EB73}';
    pub const VIEW_COMPACT: char = '\u{E42B}';
    pub const VIEW_COMPACT_ALT: char = '\u{EB74}';
    pub const VIEW_COZY: char = '\u{EB75}';
    pub const VIEW_DAY: char = '\u{E8ED}';
    pub const VIEW_HEADLINE: char = '\u{E8EE}';
    pub const VIEW_IN_AR: char = '\u{EFC9}';
    pub const VIEW_IN_AR_NEW: char = '\u{EFC9}';
    pub const VIEW_IN_AR_OFF: char = '\u{F61B}';
    pub const VIEW_KANBAN: char = '\u{EB7F}';
    pub const VIEW_LIST: char = '\u{E8EF}';
    pub const VIEW_MODULE: char = '\u{E8F0}';
    pub const VIEW_OBJECT_TRACK: char = '\u{F432}';
    pub const VIEW_QUILT: char = '\u{E8F1}';
    pub const VIEW_REAL_SIZE: char = '\u{F4C2}';
    pub const VIEW_SIDEBAR: char = '\u{F114}';
    pub const VIEW_STREAM: char = '\u{E8F2}';
    pub const VIEW_TIMELINE: char = '\u{EB85}';
    pub const VIEW_WEEK: char = '\u{E8F3}';
    pub const VIGNETTE: char = '\u{E435}';
    pub const VIGNETTE_2: char = '\u{F2B3}';
    pub const VILLA: char = '\u{E586}';
    pub const VISIBILITY: char = '\u{E8F4}';
    pub const VISIBILITY_LOCK: char = '\u{F653}';
    pub const VISIBILITY_OFF: char = '\u{E8F5}';
    pub const VITAL_SIGNS: char = '\u{E650}';
    pub const VITALS: char = '\u{E13B}';
    pub const VO2_MAX: char = '\u{F4AA}';
    pub const VOICE_CHAT: char = '\u{E62E}';
    pub const VOICE_CHAT_OFF: char = '\u{EEBE}';
    pub const VOICE_OVER_OFF: char = '\u{E94A}';
    pub const VOICE_SELECTION: char = '\u{F58A}';
    pub const VOICE_SELECTION_OFF: char = '\u{F42C}';
    pub const VOICEMAIL: char = '\u{E0D9}';
    pub const VOICEMAIL_2: char = '\u{F352}';
    pub const VOLCANO: char = '\u{EBDA}';
    pub const VOLUME_DOWN: char = '\u{E04D}';
    pub const VOLUME_DOWN_ALT: char = '\u{E79C}';
    pub const VOLUME_MUTE: char = '\u{E04E}';
    pub const VOLUME_OFF: char = '\u{E04F}';
    pub const VOLUME_UP: char = '\u{E050}';
    pub const VOLUNTEER_ACTIVISM: char = '\u{EA70}';
    pub const VOTING_CHIP: char = '\u{F852}';
    pub const VPN_KEY: char = '\u{E0DA}';
    pub const VPN_KEY_ALERT: char = '\u{F6CC}';
    pub const VPN_KEY_OFF: char = '\u{EB7A}';
    pub const VPN_LOCK: char = '\u{E62F}';
    pub const VPN_LOCK_2: char = '\u{F350}';
    pub const VR180_CREATE2D: char = '\u{EFCA}';
    pub const VR180_CREATE2D_OFF: char = '\u{F571}';
    pub const VRPANO: char = '\u{F082}';
    pub const WALK_BIKE: char = '\u{FFF01}';
    pub const WALL_ART: char = '\u{EFCB}';
    pub const WALL_LAMP: char = '\u{E2B4}';
    pub const WALLET: char = '\u{F8FF}';
    pub const WALLPAPER: char = '\u{E1BC}';
    pub const WALLPAPER_SLIDESHOW: char = '\u{F672}';
    pub const WAND_SHINE: char = '\u{F31F}';
    pub const WAND_STARS: char = '\u{F31E}';
    pub const WARD: char = '\u{E13C}';
    pub const WAREHOUSE: char = '\u{EBB8}';
    pub const WARNING: char = '\u{F083}';
    pub const WARNING_AMBER: char = '\u{F083}';
    pub const WARNING_OFF: char = '\u{F7AD}';
    pub const WASH: char = '\u{F1B1}';
    pub const WASHOKU: char = '\u{F280}';
    pub const WATCH: char = '\u{E334}';
    pub const WATCH_ALERT: char = '\u{FFFD1}';
    pub const WATCH_ARROW: char = '\u{F2CA}';
    pub const WATCH_ARROW_DOWN: char = '\u{FFFCF}';
    pub const WATCH_BUTTON: char = '\u{FFF20}';
    pub const WATCH_BUTTON_PRESS: char = '\u{F6AA}';
    pub const WATCH_CHECK: char = '\u{F468}';
    pub const WATCH_LATER: char = '\u{EFD6}';
    pub const WATCH_LOCK: char = '\u{EEE9}';
    pub const WATCH_OFF: char = '\u{EAE3}';
    pub const WATCH_SCREENTIME: char = '\u{F6AE}';
    pub const WATCH_VIBRATION: char = '\u{F467}';
    pub const WATCH_WAKE: char = '\u{F6A9}';
    pub const WATER: char = '\u{F084}';
    pub const WATER_BOTTLE: char = '\u{F69D}';
    pub const WATER_BOTTLE_LARGE: char = '\u{F69E}';
    pub const WATER_DAMAGE: char = '\u{F203}';
    pub const WATER_DO: char = '\u{F870}';
    pub const WATER_DROP: char = '\u{E798}';
    pub const WATER_DROPS: char = '\u{FFFA5}';
    pub const WATER_EC: char = '\u{F875}';
    pub const WATER_FULL: char = '\u{F6D6}';
    pub const WATER_HEATER: char = '\u{E284}';
    pub const WATER_LOCK: char = '\u{F6AD}';
    pub const WATER_LOSS: char = '\u{F6D5}';
    pub const WATER_LUX: char = '\u{F874}';
    pub const WATER_MEDIUM: char = '\u{F6D4}';
    pub const WATER_ORP: char = '\u{F878}';
    pub const WATER_PH: char = '\u{F87A}';
    pub const WATER_PUMP: char = '\u{F5D8}';
    pub const WATER_VOC: char = '\u{F87B}';
    pub const WATERFALL_CHART: char = '\u{EA00}';
    pub const WAVES: char = '\u{E176}';
    pub const WAVING_HAND: char = '\u{E766}';
    pub const WB_AUTO: char = '\u{E42C}';
    pub const WB_CLOUDY: char = '\u{F15C}';
    pub const WB_INCANDESCENT: char = '\u{E42E}';
    pub const WB_IRIDESCENT: char = '\u{F07D}';
    pub const WB_SHADE: char = '\u{EA01}';
    pub const WB_SUNNY: char = '\u{E430}';
    pub const WB_TWILIGHT: char = '\u{E1C6}';
    pub const WB_TWILIGHT_2: char = '\u{FFF1F}';
    pub const WC: char = '\u{E63D}';
    pub const WEATHER_HAIL: char = '\u{F67F}';
    pub const WEATHER_MIX: char = '\u{F60B}';
    pub const WEATHER_SNOWY: char = '\u{E2CD}';
    pub const WEB: char = '\u{E66A}';
    pub const WEB_ASSET: char = '\u{E069}';
    pub const WEB_ASSET_OFF: char = '\u{EF47}';
    pub const WEB_STORIES: char = '\u{E595}';
    pub const WEB_TRAFFIC: char = '\u{EA03}';
    pub const WEBHOOK: char = '\u{EB92}';
    pub const WEEKEND: char = '\u{E16B}';
    pub const WEIGHT: char = '\u{E13D}';
    pub const WEST: char = '\u{F1E6}';
    pub const WHATSHOT: char = '\u{E80E}';
    pub const WHEAT: char = '\u{FFFA4}';
    pub const WHEELCHAIR_PICKUP: char = '\u{F1AB}';
    pub const WHERE_TO_VOTE: char = '\u{E177}';
    pub const WIDGET_MEDIUM: char = '\u{F3BA}';
    pub const WIDGET_MENU: char = '\u{EEB7}';
    pub const WIDGET_SMALL: char = '\u{F3B9}';
    pub const WIDGET_WIDTH: char = '\u{F3B8}';
    pub const WIDGETS: char = '\u{E1BD}';
    pub const WIDTH: char = '\u{F730}';
    pub const WIDTH_FULL: char = '\u{F8F5}';
    pub const WIDTH_NORMAL: char = '\u{F8F6}';
    pub const WIDTH_WIDE: char = '\u{F8F7}';
    pub const WIFI: char = '\u{E63E}';
    pub const WIFI_1_BAR: char = '\u{E4CA}';
    pub const WIFI_2_BAR: char = '\u{E4D9}';
    pub const WIFI_ADD: char = '\u{F7A8}';
    pub const WIFI_CALLING: char = '\u{EF77}';
    pub const WIFI_CALLING_1: char = '\u{F0E7}';
    pub const WIFI_CALLING_2: char = '\u{F0F6}';
    pub const WIFI_CALLING_3: char = '\u{F0E7}';
    pub const WIFI_CALLING_BAR_1: char = '\u{F44C}';
    pub const WIFI_CALLING_BAR_2: char = '\u{F44B}';
    pub const WIFI_CALLING_BAR_3: char = '\u{F44A}';
    pub const WIFI_CHANNEL: char = '\u{EB6A}';
    pub const WIFI_DEVICE: char = '\u{FFF34}';
    pub const WIFI_FIND: char = '\u{EB31}';
    pub const WIFI_HOME: char = '\u{F671}';
    pub const WIFI_LOCK: char = '\u{E1E1}';
    pub const WIFI_NOTIFICATION: char = '\u{F670}';
    pub const WIFI_OFF: char = '\u{E648}';
    pub const WIFI_PASSWORD: char = '\u{EB6B}';
    pub const WIFI_PROTECTED_SETUP: char = '\u{F0FC}';
    pub const WIFI_PROXY: char = '\u{F7A7}';
    pub const WIFI_TETHERING: char = '\u{E1E2}';
    pub const WIFI_TETHERING_ERROR: char = '\u{EAD9}';
    pub const WIFI_TETHERING_OFF: char = '\u{F087}';
    pub const WIND_POWER: char = '\u{EC0C}';
    pub const WINDOW: char = '\u{F088}';
    pub const WINDOW_CLOSED: char = '\u{E77E}';
    pub const WINDOW_OPEN: char = '\u{E78C}';
    pub const WINDOW_SENSOR: char = '\u{E2BB}';
    pub const WINDSHIELD_DEFROST_AUTO: char = '\u{F248}';
    pub const WINDSHIELD_DEFROST_FRONT: char = '\u{F32A}';
    pub const WINDSHIELD_DEFROST_REAR: char = '\u{F329}';
    pub const WINDSHIELD_HEAT_FRONT: char = '\u{F328}';
    pub const WINE_BAR: char = '\u{F1E8}';
    pub const WOMAN: char = '\u{E13E}';
    pub const WOMAN_2: char = '\u{F8E7}';
    pub const WORK: char = '\u{E943}';
    pub const WORK_ALERT: char = '\u{F5F7}';
    pub const WORK_HISTORY: char = '\u{EC09}';
    pub const WORK_OFF: char = '\u{E942}';
    pub const WORK_OUTLINE: char = '\u{E943}';
    pub const WORK_UPDATE: char = '\u{F5F8}';
    pub const WORKFLOW: char = '\u{EA04}';
    pub const WORKSPACE_PREMIUM: char = '\u{E7AF}';
    pub const WORKSPACES: char = '\u{EA0F}';
    pub const WORKSPACES_OUTLINE: char = '\u{EA0F}';
    pub const WOUNDS_INJURIES: char = '\u{E13F}';
    pub const WRAP_TEXT: char = '\u{E25B}';
    pub const WRIST: char = '\u{F69C}';
    pub const WRONG_LOCATION: char = '\u{EF78}';
    pub const WYSIWYG: char = '\u{F1C3}';
    pub const X_CIRCLE: char = '\u{E349}';
    pub const Y_CIRCLE: char = '\u{EEC5}';
    pub const YAKITORI: char = '\u{EF31}';
    pub const YARD: char = '\u{F089}';
    pub const YOSHOKU: char = '\u{F27F}';
    pub const YOUR_TRIPS: char = '\u{EB2B}';
    pub const YOUTUBE_ACTIVITY: char = '\u{F85A}';
    pub const YOUTUBE_SEARCHED_FOR: char = '\u{E8FA}';
    pub const ZONE_PERSON_ALERT: char = '\u{E781}';
    pub const ZONE_PERSON_IDLE: char = '\u{E77A}';
    pub const ZONE_PERSON_URGENT: char = '\u{E788}';
    pub const ZOOM_IN: char = '\u{E8FF}';
    pub const ZOOM_IN_MAP: char = '\u{EB2D}';
    pub const ZOOM_OUT: char = '\u{E900}';
    pub const ZOOM_OUT_MAP: char = '\u{E56B}';
}

#[rustfmt::skip]
static TABLE: &[(&str, char)] = &[
    ("10k", sym::_10K),
    ("10mp", sym::_10MP),
    ("11mp", sym::_11MP),
    ("123", sym::_123),
    ("12mp", sym::_12MP),
    ("13mp", sym::_13MP),
    ("14mp", sym::_14MP),
    ("15mp", sym::_15MP),
    ("16mp", sym::_16MP),
    ("17mp", sym::_17MP),
    ("18_up_rating", sym::_18_UP_RATING),
    ("18mp", sym::_18MP),
    ("19mp", sym::_19MP),
    ("1k", sym::_1K),
    ("1k_plus", sym::_1K_PLUS),
    ("1x_mobiledata", sym::_1X_MOBILEDATA),
    ("1x_mobiledata_badge", sym::_1X_MOBILEDATA_BADGE),
    ("20mp", sym::_20MP),
    ("21mp", sym::_21MP),
    ("22mp", sym::_22MP),
    ("23mp", sym::_23MP),
    ("24fps_select", sym::_24FPS_SELECT),
    ("24mp", sym::_24MP),
    ("2d", sym::_2D),
    ("2d_2", sym::_2D_2),
    ("2k", sym::_2K),
    ("2k_plus", sym::_2K_PLUS),
    ("2mp", sym::_2MP),
    ("30fps", sym::_30FPS),
    ("30fps_select", sym::_30FPS_SELECT),
    ("360", sym::_360),
    ("3d", sym::_3D),
    ("3d_2", sym::_3D_2),
    ("3d_rotation", sym::_3D_ROTATION),
    ("3g_mobiledata", sym::_3G_MOBILEDATA),
    ("3g_mobiledata_badge", sym::_3G_MOBILEDATA_BADGE),
    ("3k", sym::_3K),
    ("3k_plus", sym::_3K_PLUS),
    ("3mp", sym::_3MP),
    ("3p", sym::_3P),
    ("4g_mobiledata", sym::_4G_MOBILEDATA),
    ("4g_mobiledata_badge", sym::_4G_MOBILEDATA_BADGE),
    ("4g_plus_mobiledata", sym::_4G_PLUS_MOBILEDATA),
    ("4k", sym::_4K),
    ("4k_plus", sym::_4K_PLUS),
    ("4mp", sym::_4MP),
    ("50mp", sym::_50MP),
    ("5g", sym::_5G),
    ("5g_mobiledata_badge", sym::_5G_MOBILEDATA_BADGE),
    ("5k", sym::_5K),
    ("5k_plus", sym::_5K_PLUS),
    ("5mp", sym::_5MP),
    ("60fps", sym::_60FPS),
    ("60fps_select", sym::_60FPS_SELECT),
    ("6_ft_apart", sym::_6_FT_APART),
    ("6k", sym::_6K),
    ("6k_plus", sym::_6K_PLUS),
    ("6mp", sym::_6MP),
    ("7k", sym::_7K),
    ("7k_plus", sym::_7K_PLUS),
    ("7mp", sym::_7MP),
    ("8k", sym::_8K),
    ("8k_plus", sym::_8K_PLUS),
    ("8mp", sym::_8MP),
    ("9k", sym::_9K),
    ("9k_plus", sym::_9K_PLUS),
    ("9mp", sym::_9MP),
    ("abc", sym::ABC),
    ("ac_unit", sym::AC_UNIT),
    ("access_alarm", sym::ACCESS_ALARM),
    ("access_alarms", sym::ACCESS_ALARMS),
    ("access_time", sym::ACCESS_TIME),
    ("access_time_filled", sym::ACCESS_TIME_FILLED),
    ("accessibility", sym::ACCESSIBILITY),
    ("accessibility_new", sym::ACCESSIBILITY_NEW),
    ("accessible", sym::ACCESSIBLE),
    ("accessible_forward", sym::ACCESSIBLE_FORWARD),
    ("accessible_menu", sym::ACCESSIBLE_MENU),
    ("account_balance", sym::ACCOUNT_BALANCE),
    ("account_balance_wallet", sym::ACCOUNT_BALANCE_WALLET),
    ("account_box", sym::ACCOUNT_BOX),
    ("account_child", sym::ACCOUNT_CHILD),
    ("account_child_invert", sym::ACCOUNT_CHILD_INVERT),
    ("account_circle", sym::ACCOUNT_CIRCLE),
    ("account_circle_filled", sym::ACCOUNT_CIRCLE_FILLED),
    ("account_circle_off", sym::ACCOUNT_CIRCLE_OFF),
    ("account_tree", sym::ACCOUNT_TREE),
    ("action_key", sym::ACTION_KEY),
    ("activity_zone", sym::ACTIVITY_ZONE),
    ("acupuncture", sym::ACUPUNCTURE),
    ("acute", sym::ACUTE),
    ("ad", sym::AD),
    ("ad_group", sym::AD_GROUP),
    ("ad_group_off", sym::AD_GROUP_OFF),
    ("ad_off", sym::AD_OFF),
    ("ad_units", sym::AD_UNITS),
    ("adaptive_audio_mic", sym::ADAPTIVE_AUDIO_MIC),
    ("adaptive_audio_mic_off", sym::ADAPTIVE_AUDIO_MIC_OFF),
    ("adb", sym::ADB),
    ("add", sym::ADD),
    ("add_2", sym::ADD_2),
    ("add_a_photo", sym::ADD_A_PHOTO),
    ("add_ad", sym::ADD_AD),
    ("add_alarm", sym::ADD_ALARM),
    ("add_alert", sym::ADD_ALERT),
    ("add_box", sym::ADD_BOX),
    ("add_business", sym::ADD_BUSINESS),
    ("add_call", sym::ADD_CALL),
    ("add_card", sym::ADD_CARD),
    ("add_chart", sym::ADD_CHART),
    ("add_circle", sym::ADD_CIRCLE),
    ("add_circle_outline", sym::ADD_CIRCLE_OUTLINE),
    ("add_column_left", sym::ADD_COLUMN_LEFT),
    ("add_column_right", sym::ADD_COLUMN_RIGHT),
    ("add_comment", sym::ADD_COMMENT),
    ("add_diamond", sym::ADD_DIAMOND),
    ("add_home", sym::ADD_HOME),
    ("add_home_work", sym::ADD_HOME_WORK),
    ("add_ic_call", sym::ADD_IC_CALL),
    ("add_link", sym::ADD_LINK),
    ("add_location", sym::ADD_LOCATION),
    ("add_location_alt", sym::ADD_LOCATION_ALT),
    ("add_moderator", sym::ADD_MODERATOR),
    ("add_notes", sym::ADD_NOTES),
    ("add_photo_alternate", sym::ADD_PHOTO_ALTERNATE),
    ("add_reaction", sym::ADD_REACTION),
    ("add_road", sym::ADD_ROAD),
    ("add_row_above", sym::ADD_ROW_ABOVE),
    ("add_row_below", sym::ADD_ROW_BELOW),
    ("add_shopping_cart", sym::ADD_SHOPPING_CART),
    ("add_task", sym::ADD_TASK),
    ("add_to_drive", sym::ADD_TO_DRIVE),
    ("add_to_home_screen", sym::ADD_TO_HOME_SCREEN),
    ("add_to_photos", sym::ADD_TO_PHOTOS),
    ("add_to_queue", sym::ADD_TO_QUEUE),
    ("add_triangle", sym::ADD_TRIANGLE),
    ("addchart", sym::ADDCHART),
    ("adf_scanner", sym::ADF_SCANNER),
    ("adjust", sym::ADJUST),
    ("admin_meds", sym::ADMIN_MEDS),
    ("admin_panel_settings", sym::ADMIN_PANEL_SETTINGS),
    ("ads_click", sym::ADS_CLICK),
    ("agender", sym::AGENDER),
    ("agriculture", sym::AGRICULTURE),
    ("air", sym::AIR),
    ("air_freshener", sym::AIR_FRESHENER),
    ("air_purifier", sym::AIR_PURIFIER),
    ("air_purifier_gen", sym::AIR_PURIFIER_GEN),
    ("airline_seat_flat", sym::AIRLINE_SEAT_FLAT),
    ("airline_seat_flat_angled", sym::AIRLINE_SEAT_FLAT_ANGLED),
    ("airline_seat_individual_suite", sym::AIRLINE_SEAT_INDIVIDUAL_SUITE),
    ("airline_seat_legroom_extra", sym::AIRLINE_SEAT_LEGROOM_EXTRA),
    ("airline_seat_legroom_normal", sym::AIRLINE_SEAT_LEGROOM_NORMAL),
    ("airline_seat_legroom_reduced", sym::AIRLINE_SEAT_LEGROOM_REDUCED),
    ("airline_seat_recline_extra", sym::AIRLINE_SEAT_RECLINE_EXTRA),
    ("airline_seat_recline_normal", sym::AIRLINE_SEAT_RECLINE_NORMAL),
    ("airline_stops", sym::AIRLINE_STOPS),
    ("airlines", sym::AIRLINES),
    ("airplane_ticket", sym::AIRPLANE_TICKET),
    ("airplanemode_active", sym::AIRPLANEMODE_ACTIVE),
    ("airplanemode_inactive", sym::AIRPLANEMODE_INACTIVE),
    ("airplay", sym::AIRPLAY),
    ("airport_shuttle", sym::AIRPORT_SHUTTLE),
    ("airware", sym::AIRWARE),
    ("airwave", sym::AIRWAVE),
    ("alarm", sym::ALARM),
    ("alarm_add", sym::ALARM_ADD),
    ("alarm_off", sym::ALARM_OFF),
    ("alarm_on", sym::ALARM_ON),
    ("alarm_pause", sym::ALARM_PAUSE),
    ("alarm_smart_wake", sym::ALARM_SMART_WAKE),
    ("album", sym::ALBUM),
    ("align_center", sym::ALIGN_CENTER),
    ("align_end", sym::ALIGN_END),
    ("align_flex_center", sym::ALIGN_FLEX_CENTER),
    ("align_flex_end", sym::ALIGN_FLEX_END),
    ("align_flex_start", sym::ALIGN_FLEX_START),
    ("align_horizontal_center", sym::ALIGN_HORIZONTAL_CENTER),
    ("align_horizontal_left", sym::ALIGN_HORIZONTAL_LEFT),
    ("align_horizontal_right", sym::ALIGN_HORIZONTAL_RIGHT),
    ("align_items_stretch", sym::ALIGN_ITEMS_STRETCH),
    ("align_justify_center", sym::ALIGN_JUSTIFY_CENTER),
    ("align_justify_flex_end", sym::ALIGN_JUSTIFY_FLEX_END),
    ("align_justify_flex_start", sym::ALIGN_JUSTIFY_FLEX_START),
    ("align_justify_space_around", sym::ALIGN_JUSTIFY_SPACE_AROUND),
    ("align_justify_space_between", sym::ALIGN_JUSTIFY_SPACE_BETWEEN),
    ("align_justify_space_even", sym::ALIGN_JUSTIFY_SPACE_EVEN),
    ("align_justify_stretch", sym::ALIGN_JUSTIFY_STRETCH),
    ("align_self_stretch", sym::ALIGN_SELF_STRETCH),
    ("align_space_around", sym::ALIGN_SPACE_AROUND),
    ("align_space_between", sym::ALIGN_SPACE_BETWEEN),
    ("align_space_even", sym::ALIGN_SPACE_EVEN),
    ("align_start", sym::ALIGN_START),
    ("align_stretch", sym::ALIGN_STRETCH),
    ("align_vertical_bottom", sym::ALIGN_VERTICAL_BOTTOM),
    ("align_vertical_center", sym::ALIGN_VERTICAL_CENTER),
    ("align_vertical_top", sym::ALIGN_VERTICAL_TOP),
    ("all_inbox", sym::ALL_INBOX),
    ("all_inclusive", sym::ALL_INCLUSIVE),
    ("all_match", sym::ALL_MATCH),
    ("all_out", sym::ALL_OUT),
    ("allergies", sym::ALLERGIES),
    ("allergy", sym::ALLERGY),
    ("alt_route", sym::ALT_ROUTE),
    ("alternate_email", sym::ALTERNATE_EMAIL),
    ("altitude", sym::ALTITUDE),
    ("ambient_screen", sym::AMBIENT_SCREEN),
    ("ambulance", sym::AMBULANCE),
    ("amend", sym::AMEND),
    ("amp_stories", sym::AMP_STORIES),
    ("analytics", sym::ANALYTICS),
    ("anchor", sym::ANCHOR),
    ("android", sym::ANDROID),
    ("android_cell_4_bar", sym::ANDROID_CELL_4_BAR),
    ("android_cell_4_bar_alert", sym::ANDROID_CELL_4_BAR_ALERT),
    ("android_cell_4_bar_off", sym::ANDROID_CELL_4_BAR_OFF),
    ("android_cell_4_bar_plus", sym::ANDROID_CELL_4_BAR_PLUS),
    ("android_cell_5_bar", sym::ANDROID_CELL_5_BAR),
    ("android_cell_5_bar_alert", sym::ANDROID_CELL_5_BAR_ALERT),
    ("android_cell_5_bar_off", sym::ANDROID_CELL_5_BAR_OFF),
    ("android_cell_5_bar_plus", sym::ANDROID_CELL_5_BAR_PLUS),
    ("android_cell_dual_4_bar", sym::ANDROID_CELL_DUAL_4_BAR),
    ("android_cell_dual_4_bar_alert", sym::ANDROID_CELL_DUAL_4_BAR_ALERT),
    ("android_cell_dual_4_bar_plus", sym::ANDROID_CELL_DUAL_4_BAR_PLUS),
    ("android_cell_dual_5_bar", sym::ANDROID_CELL_DUAL_5_BAR),
    ("android_cell_dual_5_bar_alert", sym::ANDROID_CELL_DUAL_5_BAR_ALERT),
    ("android_cell_dual_5_bar_plus", sym::ANDROID_CELL_DUAL_5_BAR_PLUS),
    ("android_wifi_3_bar", sym::ANDROID_WIFI_3_BAR),
    ("android_wifi_3_bar_alert", sym::ANDROID_WIFI_3_BAR_ALERT),
    ("android_wifi_3_bar_lock", sym::ANDROID_WIFI_3_BAR_LOCK),
    ("android_wifi_3_bar_off", sym::ANDROID_WIFI_3_BAR_OFF),
    ("android_wifi_3_bar_plus", sym::ANDROID_WIFI_3_BAR_PLUS),
    ("android_wifi_3_bar_question", sym::ANDROID_WIFI_3_BAR_QUESTION),
    ("android_wifi_4_bar", sym::ANDROID_WIFI_4_BAR),
    ("android_wifi_4_bar_alert", sym::ANDROID_WIFI_4_BAR_ALERT),
    ("android_wifi_4_bar_lock", sym::ANDROID_WIFI_4_BAR_LOCK),
    ("android_wifi_4_bar_off", sym::ANDROID_WIFI_4_BAR_OFF),
    ("android_wifi_4_bar_plus", sym::ANDROID_WIFI_4_BAR_PLUS),
    ("android_wifi_4_bar_question", sym::ANDROID_WIFI_4_BAR_QUESTION),
    ("animated_images", sym::ANIMATED_IMAGES),
    ("animation", sym::ANIMATION),
    ("announcement", sym::ANNOUNCEMENT),
    ("antigravity", sym::ANTIGRAVITY),
    ("aod", sym::AOD),
    ("aod_tablet", sym::AOD_TABLET),
    ("aod_watch", sym::AOD_WATCH),
    ("apartment", sym::APARTMENT),
    ("api", sym::API),
    ("apk_document", sym::APK_DOCUMENT),
    ("apk_install", sym::APK_INSTALL),
    ("app_badging", sym::APP_BADGING),
    ("app_blocking", sym::APP_BLOCKING),
    ("app_promo", sym::APP_PROMO),
    ("app_registration", sym::APP_REGISTRATION),
    ("app_settings_alt", sym::APP_SETTINGS_ALT),
    ("app_shortcut", sym::APP_SHORTCUT),
    ("apparel", sym::APPAREL),
    ("approval", sym::APPROVAL),
    ("approval_delegation", sym::APPROVAL_DELEGATION),
    ("approval_delegation_off", sym::APPROVAL_DELEGATION_OFF),
    ("apps", sym::APPS),
    ("apps_outage", sym::APPS_OUTAGE),
    ("aq", sym::AQ),
    ("aq_indoor", sym::AQ_INDOOR),
    ("ar_on_you", sym::AR_ON_YOU),
    ("ar_stickers", sym::AR_STICKERS),
    ("architecture", sym::ARCHITECTURE),
    ("archive", sym::ARCHIVE),
    ("area_chart", sym::AREA_CHART),
    ("arming_countdown", sym::ARMING_COUNTDOWN),
    ("arrow_and_edge", sym::ARROW_AND_EDGE),
    ("arrow_back", sym::ARROW_BACK),
    ("arrow_back_2", sym::ARROW_BACK_2),
    ("arrow_back_ios", sym::ARROW_BACK_IOS),
    ("arrow_back_ios_new", sym::ARROW_BACK_IOS_NEW),
    ("arrow_circle_down", sym::ARROW_CIRCLE_DOWN),
    ("arrow_circle_left", sym::ARROW_CIRCLE_LEFT),
    ("arrow_circle_right", sym::ARROW_CIRCLE_RIGHT),
    ("arrow_circle_up", sym::ARROW_CIRCLE_UP),
    ("arrow_cool_down", sym::ARROW_COOL_DOWN),
    ("arrow_downward", sym::ARROW_DOWNWARD),
    ("arrow_downward_alt", sym::ARROW_DOWNWARD_ALT),
    ("arrow_drop_down", sym::ARROW_DROP_DOWN),
    ("arrow_drop_down_circle", sym::ARROW_DROP_DOWN_CIRCLE),
    ("arrow_drop_up", sym::ARROW_DROP_UP),
    ("arrow_forward", sym::ARROW_FORWARD),
    ("arrow_forward_ios", sym::ARROW_FORWARD_IOS),
    ("arrow_insert", sym::ARROW_INSERT),
    ("arrow_left", sym::ARROW_LEFT),
    ("arrow_left_alt", sym::ARROW_LEFT_ALT),
    ("arrow_menu_close", sym::ARROW_MENU_CLOSE),
    ("arrow_menu_open", sym::ARROW_MENU_OPEN),
    ("arrow_or_edge", sym::ARROW_OR_EDGE),
    ("arrow_outward", sym::ARROW_OUTWARD),
    ("arrow_range", sym::ARROW_RANGE),
    ("arrow_right", sym::ARROW_RIGHT),
    ("arrow_right_alt", sym::ARROW_RIGHT_ALT),
    ("arrow_selector_tool", sym::ARROW_SELECTOR_TOOL),
    ("arrow_shape_up", sym::ARROW_SHAPE_UP),
    ("arrow_shape_up_stack", sym::ARROW_SHAPE_UP_STACK),
    ("arrow_shape_up_stack_2", sym::ARROW_SHAPE_UP_STACK_2),
    ("arrow_split", sym::ARROW_SPLIT),
    ("arrow_top_left", sym::ARROW_TOP_LEFT),
    ("arrow_top_right", sym::ARROW_TOP_RIGHT),
    ("arrow_upload_progress", sym::ARROW_UPLOAD_PROGRESS),
    ("arrow_upload_ready", sym::ARROW_UPLOAD_READY),
    ("arrow_upward", sym::ARROW_UPWARD),
    ("arrow_upward_alt", sym::ARROW_UPWARD_ALT),
    ("arrow_warm_up", sym::ARROW_WARM_UP),
    ("arrows_input", sym::ARROWS_INPUT),
    ("arrows_left_right_circle", sym::ARROWS_LEFT_RIGHT_CIRCLE),
    ("arrows_more_down", sym::ARROWS_MORE_DOWN),
    ("arrows_more_up", sym::ARROWS_MORE_UP),
    ("arrows_output", sym::ARROWS_OUTPUT),
    ("arrows_outward", sym::ARROWS_OUTWARD),
    ("arrows_up_down_circle", sym::ARROWS_UP_DOWN_CIRCLE),
    ("art_track", sym::ART_TRACK),
    ("article", sym::ARTICLE),
    ("article_person", sym::ARTICLE_PERSON),
    ("article_shortcut", sym::ARTICLE_SHORTCUT),
    ("artist", sym::ARTIST),
    ("aspect_ratio", sym::ASPECT_RATIO),
    ("assessment", sym::ASSESSMENT),
    ("assignment", sym::ASSIGNMENT),
    ("assignment_add", sym::ASSIGNMENT_ADD),
    ("assignment_globe", sym::ASSIGNMENT_GLOBE),
    ("assignment_ind", sym::ASSIGNMENT_IND),
    ("assignment_late", sym::ASSIGNMENT_LATE),
    ("assignment_return", sym::ASSIGNMENT_RETURN),
    ("assignment_returned", sym::ASSIGNMENT_RETURNED),
    ("assignment_turned_in", sym::ASSIGNMENT_TURNED_IN),
    ("assist_walker", sym::ASSIST_WALKER),
    ("assistant", sym::ASSISTANT),
    ("assistant_device", sym::ASSISTANT_DEVICE),
    ("assistant_direction", sym::ASSISTANT_DIRECTION),
    ("assistant_navigation", sym::ASSISTANT_NAVIGATION),
    ("assistant_on_hub", sym::ASSISTANT_ON_HUB),
    ("assistant_photo", sym::ASSISTANT_PHOTO),
    ("assured_workload", sym::ASSURED_WORKLOAD),
    ("asterisk", sym::ASTERISK),
    ("astrophotography_auto", sym::ASTROPHOTOGRAPHY_AUTO),
    ("astrophotography_off", sym::ASTROPHOTOGRAPHY_OFF),
    ("atm", sym::ATM),
    ("atr", sym::ATR),
    ("attach_email", sym::ATTACH_EMAIL),
    ("attach_file", sym::ATTACH_FILE),
    ("attach_file_add", sym::ATTACH_FILE_ADD),
    ("attach_file_off", sym::ATTACH_FILE_OFF),
    ("attach_money", sym::ATTACH_MONEY),
    ("attachment", sym::ATTACHMENT),
    ("attractions", sym::ATTRACTIONS),
    ("attribution", sym::ATTRIBUTION),
    ("audio_capture", sym::AUDIO_CAPTURE),
    ("audio_description", sym::AUDIO_DESCRIPTION),
    ("audio_file", sym::AUDIO_FILE),
    ("audio_video_receiver", sym::AUDIO_VIDEO_RECEIVER),
    ("audiotrack", sym::AUDIOTRACK),
    ("auto_activity_zone", sym::AUTO_ACTIVITY_ZONE),
    ("auto_awesome", sym::AUTO_AWESOME),
    ("auto_awesome_mosaic", sym::AUTO_AWESOME_MOSAIC),
    ("auto_awesome_motion", sym::AUTO_AWESOME_MOTION),
    ("auto_delete", sym::AUTO_DELETE),
    ("auto_detect_voice", sym::AUTO_DETECT_VOICE),
    ("auto_draw_solid", sym::AUTO_DRAW_SOLID),
    ("auto_fix", sym::AUTO_FIX),
    ("auto_fix_high", sym::AUTO_FIX_HIGH),
    ("auto_fix_normal", sym::AUTO_FIX_NORMAL),
    ("auto_fix_off", sym::AUTO_FIX_OFF),
    ("auto_graph", sym::AUTO_GRAPH),
    ("auto_label", sym::AUTO_LABEL),
    ("auto_meeting_room", sym::AUTO_MEETING_ROOM),
    ("auto_mode", sym::AUTO_MODE),
    ("auto_read_pause", sym::AUTO_READ_PAUSE),
    ("auto_read_play", sym::AUTO_READ_PLAY),
    ("auto_schedule", sym::AUTO_SCHEDULE),
    ("auto_stories", sym::AUTO_STORIES),
    ("auto_stories_off", sym::AUTO_STORIES_OFF),
    ("auto_timer", sym::AUTO_TIMER),
    ("auto_towing", sym::AUTO_TOWING),
    ("auto_transmission", sym::AUTO_TRANSMISSION),
    ("auto_videocam", sym::AUTO_VIDEOCAM),
    ("autofps_select", sym::AUTOFPS_SELECT),
    ("automation", sym::AUTOMATION),
    ("autopause", sym::AUTOPAUSE),
    ("autopay", sym::AUTOPAY),
    ("autoplay", sym::AUTOPLAY),
    ("autorenew", sym::AUTORENEW),
    ("autostop", sym::AUTOSTOP),
    ("av1", sym::AV1),
    ("av_timer", sym::AV_TIMER),
    ("avc", sym::AVC),
    ("avg_pace", sym::AVG_PACE),
    ("avg_time", sym::AVG_TIME),
    ("avocado_bean", sym::AVOCADO_BEAN),
    ("award_meal", sym::AWARD_MEAL),
    ("award_star", sym::AWARD_STAR),
    ("azm", sym::AZM),
    ("b_circle", sym::B_CIRCLE),
    ("baby_changing_station", sym::BABY_CHANGING_STATION),
    ("back_hand", sym::BACK_HAND),
    ("back_to_tab", sym::BACK_TO_TAB),
    ("background_dot_large", sym::BACKGROUND_DOT_LARGE),
    ("background_dot_small", sym::BACKGROUND_DOT_SMALL),
    ("background_grid_small", sym::BACKGROUND_GRID_SMALL),
    ("background_replace", sym::BACKGROUND_REPLACE),
    ("backlight_high", sym::BACKLIGHT_HIGH),
    ("backlight_high_off", sym::BACKLIGHT_HIGH_OFF),
    ("backlight_low", sym::BACKLIGHT_LOW),
    ("backpack", sym::BACKPACK),
    ("backspace", sym::BACKSPACE),
    ("backup", sym::BACKUP),
    ("backup_table", sym::BACKUP_TABLE),
    ("badge", sym::BADGE),
    ("badge_critical_battery", sym::BADGE_CRITICAL_BATTERY),
    ("badminton", sym::BADMINTON),
    ("bakery_dining", sym::BAKERY_DINING),
    ("balance", sym::BALANCE),
    ("balcony", sym::BALCONY),
    ("ballot", sym::BALLOT),
    ("bar_chart", sym::BAR_CHART),
    ("bar_chart_4_bars", sym::BAR_CHART_4_BARS),
    ("bar_chart_off", sym::BAR_CHART_OFF),
    ("barcode", sym::BARCODE),
    ("barcode_reader", sym::BARCODE_READER),
    ("barcode_scanner", sym::BARCODE_SCANNER),
    ("barefoot", sym::BAREFOOT),
    ("batch_prediction", sym::BATCH_PREDICTION),
    ("bath_bedrock", sym::BATH_BEDROCK),
    ("bath_outdoor", sym::BATH_OUTDOOR),
    ("bath_private", sym::BATH_PRIVATE),
    ("bath_public_large", sym::BATH_PUBLIC_LARGE),
    ("bath_soak", sym::BATH_SOAK),
    ("bathroom", sym::BATHROOM),
    ("bathtub", sym::BATHTUB),
    ("battery_0_bar", sym::BATTERY_0_BAR),
    ("battery_1_bar", sym::BATTERY_1_BAR),
    ("battery_20", sym::BATTERY_20),
    ("battery_2_bar", sym::BATTERY_2_BAR),
    ("battery_30", sym::BATTERY_30),
    ("battery_3_bar", sym::BATTERY_3_BAR),
    ("battery_4_bar", sym::BATTERY_4_BAR),
    ("battery_50", sym::BATTERY_50),
    ("battery_5_bar", sym::BATTERY_5_BAR),
    ("battery_60", sym::BATTERY_60),
    ("battery_6_bar", sym::BATTERY_6_BAR),
    ("battery_80", sym::BATTERY_80),
    ("battery_90", sym::BATTERY_90),
    ("battery_alert", sym::BATTERY_ALERT),
    ("battery_android_0", sym::BATTERY_ANDROID_0),
    ("battery_android_1", sym::BATTERY_ANDROID_1),
    ("battery_android_2", sym::BATTERY_ANDROID_2),
    ("battery_android_3", sym::BATTERY_ANDROID_3),
    ("battery_android_4", sym::BATTERY_ANDROID_4),
    ("battery_android_5", sym::BATTERY_ANDROID_5),
    ("battery_android_6", sym::BATTERY_ANDROID_6),
    ("battery_android_alert", sym::BATTERY_ANDROID_ALERT),
    ("battery_android_bolt", sym::BATTERY_ANDROID_BOLT),
    ("battery_android_frame_1", sym::BATTERY_ANDROID_FRAME_1),
    ("battery_android_frame_2", sym::BATTERY_ANDROID_FRAME_2),
    ("battery_android_frame_3", sym::BATTERY_ANDROID_FRAME_3),
    ("battery_android_frame_4", sym::BATTERY_ANDROID_FRAME_4),
    ("battery_android_frame_5", sym::BATTERY_ANDROID_FRAME_5),
    ("battery_android_frame_6", sym::BATTERY_ANDROID_FRAME_6),
    ("battery_android_frame_alert", sym::BATTERY_ANDROID_FRAME_ALERT),
    ("battery_android_frame_bolt", sym::BATTERY_ANDROID_FRAME_BOLT),
    ("battery_android_frame_full", sym::BATTERY_ANDROID_FRAME_FULL),
    ("battery_android_frame_plus", sym::BATTERY_ANDROID_FRAME_PLUS),
    ("battery_android_frame_question", sym::BATTERY_ANDROID_FRAME_QUESTION),
    ("battery_android_frame_share", sym::BATTERY_ANDROID_FRAME_SHARE),
    ("battery_android_frame_shield", sym::BATTERY_ANDROID_FRAME_SHIELD),
    ("battery_android_full", sym::BATTERY_ANDROID_FULL),
    ("battery_android_plus", sym::BATTERY_ANDROID_PLUS),
    ("battery_android_question", sym::BATTERY_ANDROID_QUESTION),
    ("battery_android_share", sym::BATTERY_ANDROID_SHARE),
    ("battery_android_shield", sym::BATTERY_ANDROID_SHIELD),
    ("battery_change", sym::BATTERY_CHANGE),
    ("battery_charging_20", sym::BATTERY_CHARGING_20),
    ("battery_charging_20_2", sym::BATTERY_CHARGING_20_2),
    ("battery_charging_30", sym::BATTERY_CHARGING_30),
    ("battery_charging_30_2", sym::BATTERY_CHARGING_30_2),
    ("battery_charging_50", sym::BATTERY_CHARGING_50),
    ("battery_charging_50_2", sym::BATTERY_CHARGING_50_2),
    ("battery_charging_60", sym::BATTERY_CHARGING_60),
    ("battery_charging_60_2", sym::BATTERY_CHARGING_60_2),
    ("battery_charging_80", sym::BATTERY_CHARGING_80),
    ("battery_charging_80_2", sym::BATTERY_CHARGING_80_2),
    ("battery_charging_90", sym::BATTERY_CHARGING_90),
    ("battery_charging_full", sym::BATTERY_CHARGING_FULL),
    ("battery_charging_full_2", sym::BATTERY_CHARGING_FULL_2),
    ("battery_error", sym::BATTERY_ERROR),
    ("battery_full", sym::BATTERY_FULL),
    ("battery_full_alt", sym::BATTERY_FULL_ALT),
    ("battery_horiz_000", sym::BATTERY_HORIZ_000),
    ("battery_horiz_050", sym::BATTERY_HORIZ_050),
    ("battery_horiz_075", sym::BATTERY_HORIZ_075),
    ("battery_low", sym::BATTERY_LOW),
    ("battery_plus", sym::BATTERY_PLUS),
    ("battery_profile", sym::BATTERY_PROFILE),
    ("battery_saver", sym::BATTERY_SAVER),
    ("battery_share", sym::BATTERY_SHARE),
    ("battery_status_good", sym::BATTERY_STATUS_GOOD),
    ("battery_std", sym::BATTERY_STD),
    ("battery_unknown", sym::BATTERY_UNKNOWN),
    ("battery_vert_005", sym::BATTERY_VERT_005),
    ("battery_vert_020", sym::BATTERY_VERT_020),
    ("battery_vert_050", sym::BATTERY_VERT_050),
    ("battery_very_low", sym::BATTERY_VERY_LOW),
    ("beach_access", sym::BEACH_ACCESS),
    ("bed", sym::BED),
    ("bedroom_baby", sym::BEDROOM_BABY),
    ("bedroom_child", sym::BEDROOM_CHILD),
    ("bedroom_parent", sym::BEDROOM_PARENT),
    ("bedtime", sym::BEDTIME),
    ("bedtime_off", sym::BEDTIME_OFF),
    ("beenhere", sym::BEENHERE),
    ("beer_meal", sym::BEER_MEAL),
    ("bento", sym::BENTO),
    ("bia", sym::BIA),
    ("bid_landscape", sym::BID_LANDSCAPE),
    ("bid_landscape_disabled", sym::BID_LANDSCAPE_DISABLED),
    ("bigtop_updates", sym::BIGTOP_UPDATES),
    ("bike_dock", sym::BIKE_DOCK),
    ("bike_lane", sym::BIKE_LANE),
    ("bike_scooter", sym::BIKE_SCOOTER),
    ("biotech", sym::BIOTECH),
    ("blanket", sym::BLANKET),
    ("blender", sym::BLENDER),
    ("blind", sym::BLIND),
    ("blinds", sym::BLINDS),
    ("blinds_2", sym::BLINDS_2),
    ("blinds_2_closed", sym::BLINDS_2_CLOSED),
    ("blinds_closed", sym::BLINDS_CLOSED),
    ("block", sym::BLOCK),
    ("blood_pressure", sym::BLOOD_PRESSURE),
    ("bloodtype", sym::BLOODTYPE),
    ("bluetooth", sym::BLUETOOTH),
    ("bluetooth_audio", sym::BLUETOOTH_AUDIO),
    ("bluetooth_connected", sym::BLUETOOTH_CONNECTED),
    ("bluetooth_disabled", sym::BLUETOOTH_DISABLED),
    ("bluetooth_drive", sym::BLUETOOTH_DRIVE),
    ("bluetooth_searching", sym::BLUETOOTH_SEARCHING),
    ("blur_circular", sym::BLUR_CIRCULAR),
    ("blur_linear", sym::BLUR_LINEAR),
    ("blur_medium", sym::BLUR_MEDIUM),
    ("blur_off", sym::BLUR_OFF),
    ("blur_on", sym::BLUR_ON),
    ("blur_short", sym::BLUR_SHORT),
    ("boat_bus", sym::BOAT_BUS),
    ("boat_railway", sym::BOAT_RAILWAY),
    ("body_fat", sym::BODY_FAT),
    ("body_system", sym::BODY_SYSTEM),
    ("bolt", sym::BOLT),
    ("bolt_boost", sym::BOLT_BOOST),
    ("bomb", sym::BOMB),
    ("book", sym::BOOK),
    ("book_2", sym::BOOK_2),
    ("book_3", sym::BOOK_3),
    ("book_4", sym::BOOK_4),
    ("book_5", sym::BOOK_5),
    ("book_6", sym::BOOK_6),
    ("book_online", sym::BOOK_ONLINE),
    ("book_ribbon", sym::BOOK_RIBBON),
    ("bookmark", sym::BOOKMARK),
    ("bookmark_add", sym::BOOKMARK_ADD),
    ("bookmark_added", sym::BOOKMARK_ADDED),
    ("bookmark_bag", sym::BOOKMARK_BAG),
    ("bookmark_border", sym::BOOKMARK_BORDER),
    ("bookmark_check", sym::BOOKMARK_CHECK),
    ("bookmark_flag", sym::BOOKMARK_FLAG),
    ("bookmark_heart", sym::BOOKMARK_HEART),
    ("bookmark_manager", sym::BOOKMARK_MANAGER),
    ("bookmark_remove", sym::BOOKMARK_REMOVE),
    ("bookmark_stacks", sym::BOOKMARK_STACKS),
    ("bookmark_star", sym::BOOKMARK_STAR),
    ("bookmarks", sym::BOOKMARKS),
    ("books_movies_and_music", sym::BOOKS_MOVIES_AND_MUSIC),
    ("border_all", sym::BORDER_ALL),
    ("border_bottom", sym::BORDER_BOTTOM),
    ("border_clear", sym::BORDER_CLEAR),
    ("border_color", sym::BORDER_COLOR),
    ("border_horizontal", sym::BORDER_HORIZONTAL),
    ("border_inner", sym::BORDER_INNER),
    ("border_left", sym::BORDER_LEFT),
    ("border_outer", sym::BORDER_OUTER),
    ("border_right", sym::BORDER_RIGHT),
    ("border_style", sym::BORDER_STYLE),
    ("border_top", sym::BORDER_TOP),
    ("border_vertical", sym::BORDER_VERTICAL),
    ("borg", sym::BORG),
    ("bottom_app_bar", sym::BOTTOM_APP_BAR),
    ("bottom_drawer", sym::BOTTOM_DRAWER),
    ("bottom_navigation", sym::BOTTOM_NAVIGATION),
    ("bottom_panel_close", sym::BOTTOM_PANEL_CLOSE),
    ("bottom_panel_open", sym::BOTTOM_PANEL_OPEN),
    ("bottom_right_click", sym::BOTTOM_RIGHT_CLICK),
    ("bottom_sheets", sym::BOTTOM_SHEETS),
    ("box", sym::BOX),
    ("box_add", sym::BOX_ADD),
    ("box_edit", sym::BOX_EDIT),
    ("boy", sym::BOY),
    ("brand_awareness", sym::BRAND_AWARENESS),
    ("brand_family", sym::BRAND_FAMILY),
    ("branding_watermark", sym::BRANDING_WATERMARK),
    ("breakfast_dining", sym::BREAKFAST_DINING),
    ("breaking_news", sym::BREAKING_NEWS),
    ("breaking_news_alt_1", sym::BREAKING_NEWS_ALT_1),
    ("breastfeeding", sym::BREASTFEEDING),
    ("brick", sym::BRICK),
    ("briefcase_meal", sym::BRIEFCASE_MEAL),
    ("brightness_1", sym::BRIGHTNESS_1),
    ("brightness_2", sym::BRIGHTNESS_2),
    ("brightness_3", sym::BRIGHTNESS_3),
    ("brightness_4", sym::BRIGHTNESS_4),
    ("brightness_5", sym::BRIGHTNESS_5),
    ("brightness_6", sym::BRIGHTNESS_6),
    ("brightness_7", sym::BRIGHTNESS_7),
    ("brightness_alert", sym::BRIGHTNESS_ALERT),
    ("brightness_auto", sym::BRIGHTNESS_AUTO),
    ("brightness_empty", sym::BRIGHTNESS_EMPTY),
    ("brightness_high", sym::BRIGHTNESS_HIGH),
    ("brightness_low", sym::BRIGHTNESS_LOW),
    ("brightness_medium", sym::BRIGHTNESS_MEDIUM),
    ("bring_your_own_ip", sym::BRING_YOUR_OWN_IP),
    ("broadcast_on_home", sym::BROADCAST_ON_HOME),
    ("broadcast_on_personal", sym::BROADCAST_ON_PERSONAL),
    ("broken_image", sym::BROKEN_IMAGE),
    ("browse", sym::BROWSE),
    ("browse_activity", sym::BROWSE_ACTIVITY),
    ("browse_gallery", sym::BROWSE_GALLERY),
    ("browser_not_supported", sym::BROWSER_NOT_SUPPORTED),
    ("browser_updated", sym::BROWSER_UPDATED),
    ("brunch_dining", sym::BRUNCH_DINING),
    ("brush", sym::BRUSH),
    ("bubble", sym::BUBBLE),
    ("bubble_chart", sym::BUBBLE_CHART),
    ("bubbles", sym::BUBBLES),
    ("bucket_check", sym::BUCKET_CHECK),
    ("bug_report", sym::BUG_REPORT),
    ("build", sym::BUILD),
    ("build_circle", sym::BUILD_CIRCLE),
    ("bullet_chart", sym::BULLET_CHART),
    ("bungalow", sym::BUNGALOW),
    ("burst_mode", sym::BURST_MODE),
    ("bus_alert", sym::BUS_ALERT),
    ("bus_map_pin", sym::BUS_MAP_PIN),
    ("bus_railway", sym::BUS_RAILWAY),
    ("business", sym::BUSINESS),
    ("business_center", sym::BUSINESS_CENTER),
    ("business_chip", sym::BUSINESS_CHIP),
    ("business_messages", sym::BUSINESS_MESSAGES),
    ("buttons_alt", sym::BUTTONS_ALT),
    ("cabin", sym::CABIN),
    ("cable", sym::CABLE),
    ("cable_car", sym::CABLE_CAR),
    ("cached", sym::CACHED),
    ("cadence", sym::CADENCE),
    ("cake", sym::CAKE),
    ("cake_add", sym::CAKE_ADD),
    ("calculate", sym::CALCULATE),
    ("calendar_add_on", sym::CALENDAR_ADD_ON),
    ("calendar_apps_script", sym::CALENDAR_APPS_SCRIPT),
    ("calendar_check", sym::CALENDAR_CHECK),
    ("calendar_clock", sym::CALENDAR_CLOCK),
    ("calendar_lock", sym::CALENDAR_LOCK),
    ("calendar_meal", sym::CALENDAR_MEAL),
    ("calendar_meal_2", sym::CALENDAR_MEAL_2),
    ("calendar_month", sym::CALENDAR_MONTH),
    ("calendar_today", sym::CALENDAR_TODAY),
    ("calendar_view_day", sym::CALENDAR_VIEW_DAY),
    ("calendar_view_month", sym::CALENDAR_VIEW_MONTH),
    ("calendar_view_week", sym::CALENDAR_VIEW_WEEK),
    ("call", sym::CALL),
    ("call_end", sym::CALL_END),
    ("call_end_alt", sym::CALL_END_ALT),
    ("call_log", sym::CALL_LOG),
    ("call_made", sym::CALL_MADE),
    ("call_merge", sym::CALL_MERGE),
    ("call_missed", sym::CALL_MISSED),
    ("call_missed_outgoing", sym::CALL_MISSED_OUTGOING),
    ("call_quality", sym::CALL_QUALITY),
    ("call_received", sym::CALL_RECEIVED),
    ("call_split", sym::CALL_SPLIT),
    ("call_to_action", sym::CALL_TO_ACTION),
    ("camera", sym::CAMERA),
    ("camera_alt", sym::CAMERA_ALT),
    ("camera_enhance", sym::CAMERA_ENHANCE),
    ("camera_front", sym::CAMERA_FRONT),
    ("camera_indoor", sym::CAMERA_INDOOR),
    ("camera_outdoor", sym::CAMERA_OUTDOOR),
    ("camera_rear", sym::CAMERA_REAR),
    ("camera_roll", sym::CAMERA_ROLL),
    ("camera_video", sym::CAMERA_VIDEO),
    ("cameraswitch", sym::CAMERASWITCH),
    ("campaign", sym::CAMPAIGN),
    ("camping", sym::CAMPING),
    ("cancel", sym::CANCEL),
    ("cancel_presentation", sym::CANCEL_PRESENTATION),
    ("cancel_schedule_send", sym::CANCEL_SCHEDULE_SEND),
    ("candle", sym::CANDLE),
    ("candlestick_chart", sym::CANDLESTICK_CHART),
    ("cannabis", sym::CANNABIS),
    ("captive_portal", sym::CAPTIVE_PORTAL),
    ("capture", sym::CAPTURE),
    ("car_crash", sym::CAR_CRASH),
    ("car_defrost_left", sym::CAR_DEFROST_LEFT),
    ("car_defrost_low_left", sym::CAR_DEFROST_LOW_LEFT),
    ("car_defrost_low_right", sym::CAR_DEFROST_LOW_RIGHT),
    ("car_defrost_mid_left", sym::CAR_DEFROST_MID_LEFT),
    ("car_defrost_mid_low_left", sym::CAR_DEFROST_MID_LOW_LEFT),
    ("car_defrost_mid_low_right", sym::CAR_DEFROST_MID_LOW_RIGHT),
    ("car_defrost_mid_right", sym::CAR_DEFROST_MID_RIGHT),
    ("car_defrost_right", sym::CAR_DEFROST_RIGHT),
    ("car_fan_low_left", sym::CAR_FAN_LOW_LEFT),
    ("car_fan_low_mid_left", sym::CAR_FAN_LOW_MID_LEFT),
    ("car_fan_low_right", sym::CAR_FAN_LOW_RIGHT),
    ("car_fan_mid_left", sym::CAR_FAN_MID_LEFT),
    ("car_fan_mid_low_right", sym::CAR_FAN_MID_LOW_RIGHT),
    ("car_fan_mid_right", sym::CAR_FAN_MID_RIGHT),
    ("car_fan_recirculate", sym::CAR_FAN_RECIRCULATE),
    ("car_fan_recirculate_2", sym::CAR_FAN_RECIRCULATE_2),
    ("car_gear", sym::CAR_GEAR),
    ("car_lock", sym::CAR_LOCK),
    ("car_mirror_heat", sym::CAR_MIRROR_HEAT),
    ("car_rental", sym::CAR_RENTAL),
    ("car_repair", sym::CAR_REPAIR),
    ("car_seat_off", sym::CAR_SEAT_OFF),
    ("car_tag", sym::CAR_TAG),
    ("card_giftcard", sym::CARD_GIFTCARD),
    ("card_membership", sym::CARD_MEMBERSHIP),
    ("card_travel", sym::CARD_TRAVEL),
    ("cardio_load", sym::CARDIO_LOAD),
    ("cardiology", sym::CARDIOLOGY),
    ("cards", sym::CARDS),
    ("cards_stack", sym::CARDS_STACK),
    ("cards_star", sym::CARDS_STAR),
    ("carpenter", sym::CARPENTER),
    ("carry_on_bag", sym::CARRY_ON_BAG),
    ("carry_on_bag_checked", sym::CARRY_ON_BAG_CHECKED),
    ("carry_on_bag_inactive", sym::CARRY_ON_BAG_INACTIVE),
    ("carry_on_bag_question", sym::CARRY_ON_BAG_QUESTION),
    ("cases", sym::CASES),
    ("casino", sym::CASINO),
    ("cast", sym::CAST),
    ("cast_connected", sym::CAST_CONNECTED),
    ("cast_for_education", sym::CAST_FOR_EDUCATION),
    ("cast_pause", sym::CAST_PAUSE),
    ("cast_warning", sym::CAST_WARNING),
    ("castle", sym::CASTLE),
    ("category", sym::CATEGORY),
    ("category_search", sym::CATEGORY_SEARCH),
    ("celebration", sym::CELEBRATION),
    ("cell_merge", sym::CELL_MERGE),
    ("cell_tower", sym::CELL_TOWER),
    ("cell_wifi", sym::CELL_WIFI),
    ("center_focus_strong", sym::CENTER_FOCUS_STRONG),
    ("center_focus_weak", sym::CENTER_FOCUS_WEAK),
    ("chair", sym::CHAIR),
    ("chair_alt", sym::CHAIR_ALT),
    ("chair_counter", sym::CHAIR_COUNTER),
    ("chair_fireplace", sym::CHAIR_FIREPLACE),
    ("chair_umbrella", sym::CHAIR_UMBRELLA),
    ("chalet", sym::CHALET),
    ("change_circle", sym::CHANGE_CIRCLE),
    ("change_history", sym::CHANGE_HISTORY),
    ("charger", sym::CHARGER),
    ("charging_station", sym::CHARGING_STATION),
    ("chart_data", sym::CHART_DATA),
    ("chat", sym::CHAT),
    ("chat_add_on", sym::CHAT_ADD_ON),
    ("chat_apps_script", sym::CHAT_APPS_SCRIPT),
    ("chat_bubble", sym::CHAT_BUBBLE),
    ("chat_bubble_off", sym::CHAT_BUBBLE_OFF),
    ("chat_bubble_outline", sym::CHAT_BUBBLE_OUTLINE),
    ("chat_dashed", sym::CHAT_DASHED),
    ("chat_error", sym::CHAT_ERROR),
    ("chat_info", sym::CHAT_INFO),
    ("chat_paste_go", sym::CHAT_PASTE_GO),
    ("chat_paste_go_2", sym::CHAT_PASTE_GO_2),
    ("check", sym::CHECK),
    ("check_alert", sym::CHECK_ALERT),
    ("check_box", sym::CHECK_BOX),
    ("check_box_outline_blank", sym::CHECK_BOX_OUTLINE_BLANK),
    ("check_circle", sym::CHECK_CIRCLE),
    ("check_circle_filled", sym::CHECK_CIRCLE_FILLED),
    ("check_circle_outline", sym::CHECK_CIRCLE_OUTLINE),
    ("check_circle_unread", sym::CHECK_CIRCLE_UNREAD),
    ("check_in_out", sym::CHECK_IN_OUT),
    ("check_indeterminate_small", sym::CHECK_INDETERMINATE_SMALL),
    ("check_small", sym::CHECK_SMALL),
    ("checkbook", sym::CHECKBOOK),
    ("checked_bag", sym::CHECKED_BAG),
    ("checked_bag_question", sym::CHECKED_BAG_QUESTION),
    ("checklist", sym::CHECKLIST),
    ("checklist_rtl", sym::CHECKLIST_RTL),
    ("checkroom", sym::CHECKROOM),
    ("cheer", sym::CHEER),
    ("chef_hat", sym::CHEF_HAT),
    ("chess", sym::CHESS),
    ("chess_bishop", sym::CHESS_BISHOP),
    ("chess_bishop_2", sym::CHESS_BISHOP_2),
    ("chess_king", sym::CHESS_KING),
    ("chess_king_2", sym::CHESS_KING_2),
    ("chess_knight", sym::CHESS_KNIGHT),
    ("chess_pawn", sym::CHESS_PAWN),
    ("chess_pawn_2", sym::CHESS_PAWN_2),
    ("chess_queen", sym::CHESS_QUEEN),
    ("chess_rook", sym::CHESS_ROOK),
    ("chevron_backward", sym::CHEVRON_BACKWARD),
    ("chevron_forward", sym::CHEVRON_FORWARD),
    ("chevron_left", sym::CHEVRON_LEFT),
    ("chevron_line_up", sym::CHEVRON_LINE_UP),
    ("chevron_right", sym::CHEVRON_RIGHT),
    ("child_care", sym::CHILD_CARE),
    ("child_friendly", sym::CHILD_FRIENDLY),
    ("child_hat", sym::CHILD_HAT),
    ("chip_extraction", sym::CHIP_EXTRACTION),
    ("chips", sym::CHIPS),
    ("chrome_reader_mode", sym::CHROME_READER_MODE),
    ("chromecast_2", sym::CHROMECAST_2),
    ("chromecast_device", sym::CHROMECAST_DEVICE),
    ("chronic", sym::CHRONIC),
    ("church", sym::CHURCH),
    ("cinematic_blur", sym::CINEMATIC_BLUR),
    ("circle", sym::CIRCLE),
    ("circle_circle", sym::CIRCLE_CIRCLE),
    ("circle_notifications", sym::CIRCLE_NOTIFICATIONS),
    ("circles", sym::CIRCLES),
    ("circles_ext", sym::CIRCLES_EXT),
    ("clarify", sym::CLARIFY),
    ("class", sym::CLASS),
    ("clean_hands", sym::CLEAN_HANDS),
    ("cleaning", sym::CLEANING),
    ("cleaning_bucket", sym::CLEANING_BUCKET),
    ("cleaning_services", sym::CLEANING_SERVICES),
    ("clear", sym::CLEAR),
    ("clear_all", sym::CLEAR_ALL),
    ("clear_day", sym::CLEAR_DAY),
    ("clear_night", sym::CLEAR_NIGHT),
    ("climate_mini_split", sym::CLIMATE_MINI_SPLIT),
    ("clinical_notes", sym::CLINICAL_NOTES),
    ("clock_arrow_down", sym::CLOCK_ARROW_DOWN),
    ("clock_arrow_up", sym::CLOCK_ARROW_UP),
    ("clock_loader_10", sym::CLOCK_LOADER_10),
    ("clock_loader_20", sym::CLOCK_LOADER_20),
    ("clock_loader_40", sym::CLOCK_LOADER_40),
    ("clock_loader_60", sym::CLOCK_LOADER_60),
    ("clock_loader_80", sym::CLOCK_LOADER_80),
    ("clock_loader_90", sym::CLOCK_LOADER_90),
    ("close", sym::CLOSE),
    ("close_fullscreen", sym::CLOSE_FULLSCREEN),
    ("close_small", sym::CLOSE_SMALL),
    ("closed_caption", sym::CLOSED_CAPTION),
    ("closed_caption_add", sym::CLOSED_CAPTION_ADD),
    ("closed_caption_disabled", sym::CLOSED_CAPTION_DISABLED),
    ("closed_caption_off", sym::CLOSED_CAPTION_OFF),
    ("cloud", sym::CLOUD),
    ("cloud_alert", sym::CLOUD_ALERT),
    ("cloud_circle", sym::CLOUD_CIRCLE),
    ("cloud_done", sym::CLOUD_DONE),
    ("cloud_download", sym::CLOUD_DOWNLOAD),
    ("cloud_lock", sym::CLOUD_LOCK),
    ("cloud_off", sym::CLOUD_OFF),
    ("cloud_queue", sym::CLOUD_QUEUE),
    ("cloud_sync", sym::CLOUD_SYNC),
    ("cloud_upload", sym::CLOUD_UPLOAD),
    ("cloudy", sym::CLOUDY),
    ("cloudy_filled", sym::CLOUDY_FILLED),
    ("cloudy_snowing", sym::CLOUDY_SNOWING),
    ("co2", sym::CO2),
    ("co_present", sym::CO_PRESENT),
    ("code", sym::CODE),
    ("code_blocks", sym::CODE_BLOCKS),
    ("code_off", sym::CODE_OFF),
    ("code_xml", sym::CODE_XML),
    ("coffee", sym::COFFEE),
    ("coffee_maker", sym::COFFEE_MAKER),
    ("cognition", sym::COGNITION),
    ("cognition_2", sym::COGNITION_2),
    ("collapse_all", sym::COLLAPSE_ALL),
    ("collapse_content", sym::COLLAPSE_CONTENT),
    ("collections", sym::COLLECTIONS),
    ("collections_bookmark", sym::COLLECTIONS_BOOKMARK),
    ("color_lens", sym::COLOR_LENS),
    ("colorize", sym::COLORIZE),
    ("colors", sym::COLORS),
    ("combine_columns", sym::COMBINE_COLUMNS),
    ("comedy_mask", sym::COMEDY_MASK),
    ("comic_bubble", sym::COMIC_BUBBLE),
    ("comment", sym::COMMENT),
    ("comment_bank", sym::COMMENT_BANK),
    ("comments_disabled", sym::COMMENTS_DISABLED),
    ("commit", sym::COMMIT),
    ("communication", sym::COMMUNICATION),
    ("communities", sym::COMMUNITIES),
    ("communities_filled", sym::COMMUNITIES_FILLED),
    ("commute", sym::COMMUTE),
    ("compare", sym::COMPARE),
    ("compare_arrows", sym::COMPARE_ARROWS),
    ("compass_calibration", sym::COMPASS_CALIBRATION),
    ("component_exchange", sym::COMPONENT_EXCHANGE),
    ("compost", sym::COMPOST),
    ("compress", sym::COMPRESS),
    ("computer", sym::COMPUTER),
    ("computer_arrow_up", sym::COMPUTER_ARROW_UP),
    ("computer_cancel", sym::COMPUTER_CANCEL),
    ("computer_sound", sym::COMPUTER_SOUND),
    ("concierge", sym::CONCIERGE),
    ("conditions", sym::CONDITIONS),
    ("confirmation_number", sym::CONFIRMATION_NUMBER),
    ("congenital", sym::CONGENITAL),
    ("connect_without_contact", sym::CONNECT_WITHOUT_CONTACT),
    ("connected_tv", sym::CONNECTED_TV),
    ("connecting_airports", sym::CONNECTING_AIRPORTS),
    ("construction", sym::CONSTRUCTION),
    ("contact_emergency", sym::CONTACT_EMERGENCY),
    ("contact_mail", sym::CONTACT_MAIL),
    ("contact_page", sym::CONTACT_PAGE),
    ("contact_phone", sym::CONTACT_PHONE),
    ("contact_phone_filled", sym::CONTACT_PHONE_FILLED),
    ("contact_support", sym::CONTACT_SUPPORT),
    ("contactless", sym::CONTACTLESS),
    ("contactless_off", sym::CONTACTLESS_OFF),
    ("contacts", sym::CONTACTS),
    ("contacts_product", sym::CONTACTS_PRODUCT),
    ("content_copy", sym::CONTENT_COPY),
    ("content_cut", sym::CONTENT_CUT),
    ("content_paste", sym::CONTENT_PASTE),
    ("content_paste_go", sym::CONTENT_PASTE_GO),
    ("content_paste_off", sym::CONTENT_PASTE_OFF),
    ("content_paste_search", sym::CONTENT_PASTE_SEARCH),
    ("contextual_token", sym::CONTEXTUAL_TOKEN),
    ("contextual_token_add", sym::CONTEXTUAL_TOKEN_ADD),
    ("contract", sym::CONTRACT),
    ("contract_delete", sym::CONTRACT_DELETE),
    ("contract_edit", sym::CONTRACT_EDIT),
    ("contrast", sym::CONTRAST),
    ("contrast_circle", sym::CONTRAST_CIRCLE),
    ("contrast_rtl_off", sym::CONTRAST_RTL_OFF),
    ("contrast_square", sym::CONTRAST_SQUARE),
    ("control_camera", sym::CONTROL_CAMERA),
    ("control_point", sym::CONTROL_POINT),
    ("control_point_duplicate", sym::CONTROL_POINT_DUPLICATE),
    ("controller_gen", sym::CONTROLLER_GEN),
    ("conversation", sym::CONVERSATION),
    ("conversion_path", sym::CONVERSION_PATH),
    ("conversion_path_off", sym::CONVERSION_PATH_OFF),
    ("convert_to_text", sym::CONVERT_TO_TEXT),
    ("conveyor_belt", sym::CONVEYOR_BELT),
    ("cookie", sym::COOKIE),
    ("cookie_off", sym::COOKIE_OFF),
    ("cooking", sym::COOKING),
    ("cool_to_dry", sym::COOL_TO_DRY),
    ("copy_all", sym::COPY_ALL),
    ("copyright", sym::COPYRIGHT),
    ("coronavirus", sym::CORONAVIRUS),
    ("corporate_fare", sym::CORPORATE_FARE),
    ("cottage", sym::COTTAGE),
    ("counter_0", sym::COUNTER_0),
    ("counter_1", sym::COUNTER_1),
    ("counter_2", sym::COUNTER_2),
    ("counter_3", sym::COUNTER_3),
    ("counter_4", sym::COUNTER_4),
    ("counter_5", sym::COUNTER_5),
    ("counter_6", sym::COUNTER_6),
    ("counter_7", sym::COUNTER_7),
    ("counter_8", sym::COUNTER_8),
    ("counter_9", sym::COUNTER_9),
    ("countertops", sym::COUNTERTOPS),
    ("create", sym::CREATE),
    ("create_new_folder", sym::CREATE_NEW_FOLDER),
    ("credit_card", sym::CREDIT_CARD),
    ("credit_card_clock", sym::CREDIT_CARD_CLOCK),
    ("credit_card_gear", sym::CREDIT_CARD_GEAR),
    ("credit_card_heart", sym::CREDIT_CARD_HEART),
    ("credit_card_off", sym::CREDIT_CARD_OFF),
    ("credit_score", sym::CREDIT_SCORE),
    ("crib", sym::CRIB),
    ("crisis_alert", sym::CRISIS_ALERT),
    ("crop", sym::CROP),
    ("crop_16_9", sym::CROP_16_9),
    ("crop_21_9", sym::CROP_21_9),
    ("crop_2_3", sym::CROP_2_3),
    ("crop_3_2", sym::CROP_3_2),
    ("crop_5_4", sym::CROP_5_4),
    ("crop_7_5", sym::CROP_7_5),
    ("crop_9_16", sym::CROP_9_16),
    ("crop_din", sym::CROP_DIN),
    ("crop_free", sym::CROP_FREE),
    ("crop_landscape", sym::CROP_LANDSCAPE),
    ("crop_original", sym::CROP_ORIGINAL),
    ("crop_portrait", sym::CROP_PORTRAIT),
    ("crop_rotate", sym::CROP_ROTATE),
    ("crop_square", sym::CROP_SQUARE),
    ("crossword", sym::CROSSWORD),
    ("crowdsource", sym::CROWDSOURCE),
    ("crown", sym::CROWN),
    ("cruelty_free", sym::CRUELTY_FREE),
    ("css", sym::CSS),
    ("csv", sym::CSV),
    ("currency_bitcoin", sym::CURRENCY_BITCOIN),
    ("currency_exchange", sym::CURRENCY_EXCHANGE),
    ("currency_franc", sym::CURRENCY_FRANC),
    ("currency_lira", sym::CURRENCY_LIRA),
    ("currency_pound", sym::CURRENCY_POUND),
    ("currency_ruble", sym::CURRENCY_RUBLE),
    ("currency_rupee", sym::CURRENCY_RUPEE),
    ("currency_rupee_circle", sym::CURRENCY_RUPEE_CIRCLE),
    ("currency_yen", sym::CURRENCY_YEN),
    ("currency_yuan", sym::CURRENCY_YUAN),
    ("curtains", sym::CURTAINS),
    ("curtains_closed", sym::CURTAINS_CLOSED),
    ("custom_typography", sym::CUSTOM_TYPOGRAPHY),
    ("cut", sym::CUT),
    ("cycle", sym::CYCLE),
    ("cyclone", sym::CYCLONE),
    ("dangerous", sym::DANGEROUS),
    ("dark_mode", sym::DARK_MODE),
    ("dashboard", sym::DASHBOARD),
    ("dashboard_2", sym::DASHBOARD_2),
    ("dashboard_2_add", sym::DASHBOARD_2_ADD),
    ("dashboard_2_edit", sym::DASHBOARD_2_EDIT),
    ("dashboard_2_gear", sym::DASHBOARD_2_GEAR),
    ("dashboard_customize", sym::DASHBOARD_CUSTOMIZE),
    ("data_alert", sym::DATA_ALERT),
    ("data_array", sym::DATA_ARRAY),
    ("data_check", sym::DATA_CHECK),
    ("data_exploration", sym::DATA_EXPLORATION),
    ("data_info_alert", sym::DATA_INFO_ALERT),
    ("data_loss_prevention", sym::DATA_LOSS_PREVENTION),
    ("data_object", sym::DATA_OBJECT),
    ("data_saver_off", sym::DATA_SAVER_OFF),
    ("data_saver_on", sym::DATA_SAVER_ON),
    ("data_table", sym::DATA_TABLE),
    ("data_thresholding", sym::DATA_THRESHOLDING),
    ("data_usage", sym::DATA_USAGE),
    ("database", sym::DATABASE),
    ("database_off", sym::DATABASE_OFF),
    ("database_search", sym::DATABASE_SEARCH),
    ("database_upload", sym::DATABASE_UPLOAD),
    ("dataset", sym::DATASET),
    ("dataset_linked", sym::DATASET_LINKED),
    ("date_range", sym::DATE_RANGE),
    ("deblur", sym::DEBLUR),
    ("deceased", sym::DECEASED),
    ("decimal_decrease", sym::DECIMAL_DECREASE),
    ("decimal_increase", sym::DECIMAL_INCREASE),
    ("deck", sym::DECK),
    ("dehaze", sym::DEHAZE),
    ("delete", sym::DELETE),
    ("delete_forever", sym::DELETE_FOREVER),
    ("delete_history", sym::DELETE_HISTORY),
    ("delete_outline", sym::DELETE_OUTLINE),
    ("delete_sweep", sym::DELETE_SWEEP),
    ("delivery_dining", sym::DELIVERY_DINING),
    ("delivery_truck_bolt", sym::DELIVERY_TRUCK_BOLT),
    ("delivery_truck_speed", sym::DELIVERY_TRUCK_SPEED),
    ("demography", sym::DEMOGRAPHY),
    ("density_large", sym::DENSITY_LARGE),
    ("density_medium", sym::DENSITY_MEDIUM),
    ("density_small", sym::DENSITY_SMALL),
    ("dentistry", sym::DENTISTRY),
    ("departure_board", sym::DEPARTURE_BOARD),
    ("deployed_code", sym::DEPLOYED_CODE),
    ("deployed_code_account", sym::DEPLOYED_CODE_ACCOUNT),
    ("deployed_code_alert", sym::DEPLOYED_CODE_ALERT),
    ("deployed_code_history", sym::DEPLOYED_CODE_HISTORY),
    ("deployed_code_update", sym::DEPLOYED_CODE_UPDATE),
    ("dermatology", sym::DERMATOLOGY),
    ("description", sym::DESCRIPTION),
    ("deselect", sym::DESELECT),
    ("design_services", sym::DESIGN_SERVICES),
    ("desk", sym::DESK),
    ("deskphone", sym::DESKPHONE),
    ("desktop_access_disabled", sym::DESKTOP_ACCESS_DISABLED),
    ("desktop_cloud", sym::DESKTOP_CLOUD),
    ("desktop_cloud_stack", sym::DESKTOP_CLOUD_STACK),
    ("desktop_landscape", sym::DESKTOP_LANDSCAPE),
    ("desktop_landscape_add", sym::DESKTOP_LANDSCAPE_ADD),
    ("desktop_mac", sym::DESKTOP_MAC),
    ("desktop_portrait", sym::DESKTOP_PORTRAIT),
    ("desktop_windows", sym::DESKTOP_WINDOWS),
    ("destruction", sym::DESTRUCTION),
    ("details", sym::DETAILS),
    ("detection_and_zone", sym::DETECTION_AND_ZONE),
    ("detection_and_zone_off", sym::DETECTION_AND_ZONE_OFF),
    ("detector", sym::DETECTOR),
    ("detector_alarm", sym::DETECTOR_ALARM),
    ("detector_battery", sym::DETECTOR_BATTERY),
    ("detector_co", sym::DETECTOR_CO),
    ("detector_offline", sym::DETECTOR_OFFLINE),
    ("detector_smoke", sym::DETECTOR_SMOKE),
    ("detector_status", sym::DETECTOR_STATUS),
    ("developer_board", sym::DEVELOPER_BOARD),
    ("developer_board_off", sym::DEVELOPER_BOARD_OFF),
    ("developer_guide", sym::DEVELOPER_GUIDE),
    ("developer_mode", sym::DEVELOPER_MODE),
    ("developer_mode_tv", sym::DEVELOPER_MODE_TV),
    ("device_band", sym::DEVICE_BAND),
    ("device_hub", sym::DEVICE_HUB),
    ("device_reset", sym::DEVICE_RESET),
    ("device_swoosh_star", sym::DEVICE_SWOOSH_STAR),
    ("device_thermostat", sym::DEVICE_THERMOSTAT),
    ("device_unknown", sym::DEVICE_UNKNOWN),
    ("devices", sym::DEVICES),
    ("devices_fold", sym::DEVICES_FOLD),
    ("devices_fold_2", sym::DEVICES_FOLD_2),
    ("devices_off", sym::DEVICES_OFF),
    ("devices_other", sym::DEVICES_OTHER),
    ("devices_wearables", sym::DEVICES_WEARABLES),
    ("dew_point", sym::DEW_POINT),
    ("diagnosis", sym::DIAGNOSIS),
    ("diagonal_line", sym::DIAGONAL_LINE),
    ("dialer_sip", sym::DIALER_SIP),
    ("dialogs", sym::DIALOGS),
    ("dialpad", sym::DIALPAD),
    ("diamond", sym::DIAMOND),
    ("diamond_shine", sym::DIAMOND_SHINE),
    ("dictionary", sym::DICTIONARY),
    ("difference", sym::DIFFERENCE),
    ("digital_out_of_home", sym::DIGITAL_OUT_OF_HOME),
    ("digital_wellbeing", sym::DIGITAL_WELLBEING),
    ("dine_heart", sym::DINE_HEART),
    ("dine_in", sym::DINE_IN),
    ("dine_lamp", sym::DINE_LAMP),
    ("dining", sym::DINING),
    ("dinner_dining", sym::DINNER_DINING),
    ("directions", sym::DIRECTIONS),
    ("directions_alt", sym::DIRECTIONS_ALT),
    ("directions_alt_off", sym::DIRECTIONS_ALT_OFF),
    ("directions_bike", sym::DIRECTIONS_BIKE),
    ("directions_boat", sym::DIRECTIONS_BOAT),
    ("directions_boat_filled", sym::DIRECTIONS_BOAT_FILLED),
    ("directions_bus", sym::DIRECTIONS_BUS),
    ("directions_bus_filled", sym::DIRECTIONS_BUS_FILLED),
    ("directions_car", sym::DIRECTIONS_CAR),
    ("directions_car_filled", sym::DIRECTIONS_CAR_FILLED),
    ("directions_off", sym::DIRECTIONS_OFF),
    ("directions_railway", sym::DIRECTIONS_RAILWAY),
    ("directions_railway_2", sym::DIRECTIONS_RAILWAY_2),
    ("directions_railway_filled", sym::DIRECTIONS_RAILWAY_FILLED),
    ("directions_run", sym::DIRECTIONS_RUN),
    ("directions_subway", sym::DIRECTIONS_SUBWAY),
    ("directions_subway_filled", sym::DIRECTIONS_SUBWAY_FILLED),
    ("directions_transit", sym::DIRECTIONS_TRANSIT),
    ("directions_transit_filled", sym::DIRECTIONS_TRANSIT_FILLED),
    ("directions_walk", sym::DIRECTIONS_WALK),
    ("directory_sync", sym::DIRECTORY_SYNC),
    ("dirty_lens", sym::DIRTY_LENS),
    ("disabled_by_default", sym::DISABLED_BY_DEFAULT),
    ("disabled_visible", sym::DISABLED_VISIBLE),
    ("disc_full", sym::DISC_FULL),
    ("discover_tune", sym::DISCOVER_TUNE),
    ("dishwasher", sym::DISHWASHER),
    ("dishwasher_gen", sym::DISHWASHER_GEN),
    ("display_add", sym::DISPLAY_ADD),
    ("display_external_input", sym::DISPLAY_EXTERNAL_INPUT),
    ("display_settings", sym::DISPLAY_SETTINGS),
    ("distance", sym::DISTANCE),
    ("diversity_1", sym::DIVERSITY_1),
    ("diversity_2", sym::DIVERSITY_2),
    ("diversity_3", sym::DIVERSITY_3),
    ("diversity_4", sym::DIVERSITY_4),
    ("dns", sym::DNS),
    ("do_disturb", sym::DO_DISTURB),
    ("do_disturb_alt", sym::DO_DISTURB_ALT),
    ("do_disturb_off", sym::DO_DISTURB_OFF),
    ("do_disturb_on", sym::DO_DISTURB_ON),
    ("do_not_disturb", sym::DO_NOT_DISTURB),
    ("do_not_disturb_alt", sym::DO_NOT_DISTURB_ALT),
    ("do_not_disturb_off", sym::DO_NOT_DISTURB_OFF),
    ("do_not_disturb_on", sym::DO_NOT_DISTURB_ON),
    ("do_not_disturb_on_total_silence", sym::DO_NOT_DISTURB_ON_TOTAL_SILENCE),
    ("do_not_step", sym::DO_NOT_STEP),
    ("do_not_touch", sym::DO_NOT_TOUCH),
    ("dock", sym::DOCK),
    ("dock_to_bottom", sym::DOCK_TO_BOTTOM),
    ("dock_to_left", sym::DOCK_TO_LEFT),
    ("dock_to_right", sym::DOCK_TO_RIGHT),
    ("docs", sym::DOCS),
    ("docs_add_on", sym::DOCS_ADD_ON),
    ("docs_apps_script", sym::DOCS_APPS_SCRIPT),
    ("document_scanner", sym::DOCUMENT_SCANNER),
    ("document_search", sym::DOCUMENT_SEARCH),
    ("domain", sym::DOMAIN),
    ("domain_add", sym::DOMAIN_ADD),
    ("domain_disabled", sym::DOMAIN_DISABLED),
    ("domain_disabled_check", sym::DOMAIN_DISABLED_CHECK),
    ("domain_verification", sym::DOMAIN_VERIFICATION),
    ("domain_verification_off", sym::DOMAIN_VERIFICATION_OFF),
    ("domino_mask", sym::DOMINO_MASK),
    ("done", sym::DONE),
    ("done_all", sym::DONE_ALL),
    ("done_outline", sym::DONE_OUTLINE),
    ("donut_large", sym::DONUT_LARGE),
    ("donut_small", sym::DONUT_SMALL),
    ("door_back", sym::DOOR_BACK),
    ("door_front", sym::DOOR_FRONT),
    ("door_open", sym::DOOR_OPEN),
    ("door_sensor", sym::DOOR_SENSOR),
    ("door_sliding", sym::DOOR_SLIDING),
    ("doorbell", sym::DOORBELL),
    ("doorbell_3p", sym::DOORBELL_3P),
    ("doorbell_chime", sym::DOORBELL_CHIME),
    ("double_arrow", sym::DOUBLE_ARROW),
    ("downhill_skiing", sym::DOWNHILL_SKIING),
    ("download", sym::DOWNLOAD),
    ("download_2", sym::DOWNLOAD_2),
    ("download_done", sym::DOWNLOAD_DONE),
    ("download_for_offline", sym::DOWNLOAD_FOR_OFFLINE),
    ("downloading", sym::DOWNLOADING),
    ("draft", sym::DRAFT),
    ("draft_orders", sym::DRAFT_ORDERS),
    ("drafts", sym::DRAFTS),
    ("drag_click", sym::DRAG_CLICK),
    ("drag_handle", sym::DRAG_HANDLE),
    ("drag_indicator", sym::DRAG_INDICATOR),
    ("drag_pan", sym::DRAG_PAN),
    ("draw", sym::DRAW),
    ("draw_abstract", sym::DRAW_ABSTRACT),
    ("draw_collage", sym::DRAW_COLLAGE),
    ("drawing_recognition", sym::DRAWING_RECOGNITION),
    ("dresser", sym::DRESSER),
    ("drive_eta", sym::DRIVE_ETA),
    ("drive_export", sym::DRIVE_EXPORT),
    ("drive_file_move", sym::DRIVE_FILE_MOVE),
    ("drive_file_move_outline", sym::DRIVE_FILE_MOVE_OUTLINE),
    ("drive_file_move_rtl", sym::DRIVE_FILE_MOVE_RTL),
    ("drive_file_rename", sym::DRIVE_FILE_RENAME),
    ("drive_file_rename_outline", sym::DRIVE_FILE_RENAME_OUTLINE),
    ("drive_folder_upload", sym::DRIVE_FOLDER_UPLOAD),
    ("drone", sym::DRONE),
    ("drone_2", sym::DRONE_2),
    ("dropdown", sym::DROPDOWN),
    ("dropdown_menu", sym::DROPDOWN_MENU),
    ("dropper_eye", sym::DROPPER_EYE),
    ("dry", sym::DRY),
    ("dry_cleaning", sym::DRY_CLEANING),
    ("dual_screen", sym::DUAL_SCREEN),
    ("duo", sym::DUO),
    ("dvr", sym::DVR),
    ("dynamic_feed", sym::DYNAMIC_FEED),
    ("dynamic_form", sym::DYNAMIC_FORM),
    ("e911_avatar", sym::E911_AVATAR),
    ("e911_emergency", sym::E911_EMERGENCY),
    ("e_mobiledata", sym::E_MOBILEDATA),
    ("e_mobiledata_badge", sym::E_MOBILEDATA_BADGE),
    ("ear_sound", sym::EAR_SOUND),
    ("earbud_case", sym::EARBUD_CASE),
    ("earbud_left", sym::EARBUD_LEFT),
    ("earbud_right", sym::EARBUD_RIGHT),
    ("earbuds", sym::EARBUDS),
    ("earbuds_2", sym::EARBUDS_2),
    ("earbuds_battery", sym::EARBUDS_BATTERY),
    ("early_on", sym::EARLY_ON),
    ("earthquake", sym::EARTHQUAKE),
    ("east", sym::EAST),
    ("ecg", sym::ECG),
    ("ecg_heart", sym::ECG_HEART),
    ("eco", sym::ECO),
    ("eda", sym::EDA),
    ("edgesensor_high", sym::EDGESENSOR_HIGH),
    ("edgesensor_low", sym::EDGESENSOR_LOW),
    ("edit", sym::EDIT),
    ("edit_arrow_down", sym::EDIT_ARROW_DOWN),
    ("edit_arrow_up", sym::EDIT_ARROW_UP),
    ("edit_attributes", sym::EDIT_ATTRIBUTES),
    ("edit_audio", sym::EDIT_AUDIO),
    ("edit_calendar", sym::EDIT_CALENDAR),
    ("edit_document", sym::EDIT_DOCUMENT),
    ("edit_location", sym::EDIT_LOCATION),
    ("edit_location_alt", sym::EDIT_LOCATION_ALT),
    ("edit_note", sym::EDIT_NOTE),
    ("edit_notifications", sym::EDIT_NOTIFICATIONS),
    ("edit_off", sym::EDIT_OFF),
    ("edit_road", sym::EDIT_ROAD),
    ("edit_square", sym::EDIT_SQUARE),
    ("editor_choice", sym::EDITOR_CHOICE),
    ("egg", sym::EGG),
    ("egg_alt", sym::EGG_ALT),
    ("eject", sym::EJECT),
    ("elderly", sym::ELDERLY),
    ("elderly_woman", sym::ELDERLY_WOMAN),
    ("electric_bike", sym::ELECTRIC_BIKE),
    ("electric_bolt", sym::ELECTRIC_BOLT),
    ("electric_car", sym::ELECTRIC_CAR),
    ("electric_meter", sym::ELECTRIC_METER),
    ("electric_moped", sym::ELECTRIC_MOPED),
    ("electric_rickshaw", sym::ELECTRIC_RICKSHAW),
    ("electric_scooter", sym::ELECTRIC_SCOOTER),
    ("electrical_services", sym::ELECTRICAL_SERVICES),
    ("elevation", sym::ELEVATION),
    ("elevator", sym::ELEVATOR),
    ("email", sym::EMAIL),
    ("emergency", sym::EMERGENCY),
    ("emergency_heat", sym::EMERGENCY_HEAT),
    ("emergency_heat_2", sym::EMERGENCY_HEAT_2),
    ("emergency_home", sym::EMERGENCY_HOME),
    ("emergency_recording", sym::EMERGENCY_RECORDING),
    ("emergency_share", sym::EMERGENCY_SHARE),
    ("emergency_share_off", sym::EMERGENCY_SHARE_OFF),
    ("emoji_emotions", sym::EMOJI_EMOTIONS),
    ("emoji_events", sym::EMOJI_EVENTS),
    ("emoji_flags", sym::EMOJI_FLAGS),
    ("emoji_food_beverage", sym::EMOJI_FOOD_BEVERAGE),
    ("emoji_language", sym::EMOJI_LANGUAGE),
    ("emoji_nature", sym::EMOJI_NATURE),
    ("emoji_objects", sym::EMOJI_OBJECTS),
    ("emoji_people", sym::EMOJI_PEOPLE),
    ("emoji_symbols", sym::EMOJI_SYMBOLS),
    ("emoji_transportation", sym::EMOJI_TRANSPORTATION),
    ("emoticon", sym::EMOTICON),
    ("empty_dashboard", sym::EMPTY_DASHBOARD),
    ("enable", sym::ENABLE),
    ("encrypted", sym::ENCRYPTED),
    ("encrypted_add", sym::ENCRYPTED_ADD),
    ("encrypted_add_circle", sym::ENCRYPTED_ADD_CIRCLE),
    ("encrypted_minus_circle", sym::ENCRYPTED_MINUS_CIRCLE),
    ("encrypted_off", sym::ENCRYPTED_OFF),
    ("endocrinology", sym::ENDOCRINOLOGY),
    ("energy", sym::ENERGY),
    ("energy_program_saving", sym::ENERGY_PROGRAM_SAVING),
    ("energy_program_time_used", sym::ENERGY_PROGRAM_TIME_USED),
    ("energy_savings_leaf", sym::ENERGY_SAVINGS_LEAF),
    ("engineering", sym::ENGINEERING),
    ("enhanced_encryption", sym::ENHANCED_ENCRYPTION),
    ("ent", sym::ENT),
    ("enterprise", sym::ENTERPRISE),
    ("enterprise_off", sym::ENTERPRISE_OFF),
    ("equal", sym::EQUAL),
    ("equalizer", sym::EQUALIZER),
    ("eraser_size_1", sym::ERASER_SIZE_1),
    ("eraser_size_2", sym::ERASER_SIZE_2),
    ("eraser_size_3", sym::ERASER_SIZE_3),
    ("eraser_size_4", sym::ERASER_SIZE_4),
    ("eraser_size_5", sym::ERASER_SIZE_5),
    ("error", sym::ERROR),
    ("error_circle_rounded", sym::ERROR_CIRCLE_ROUNDED),
    ("error_med", sym::ERROR_MED),
    ("error_outline", sym::ERROR_OUTLINE),
    ("escalator", sym::ESCALATOR),
    ("escalator_warning", sym::ESCALATOR_WARNING),
    ("euro", sym::EURO),
    ("euro_symbol", sym::EURO_SYMBOL),
    ("ev_charger", sym::EV_CHARGER),
    ("ev_mobiledata_badge", sym::EV_MOBILEDATA_BADGE),
    ("ev_shadow", sym::EV_SHADOW),
    ("ev_shadow_add", sym::EV_SHADOW_ADD),
    ("ev_shadow_minus", sym::EV_SHADOW_MINUS),
    ("ev_station", sym::EV_STATION),
    ("event", sym::EVENT),
    ("event_available", sym::EVENT_AVAILABLE),
    ("event_busy", sym::EVENT_BUSY),
    ("event_list", sym::EVENT_LIST),
    ("event_note", sym::EVENT_NOTE),
    ("event_repeat", sym::EVENT_REPEAT),
    ("event_seat", sym::EVENT_SEAT),
    ("event_upcoming", sym::EVENT_UPCOMING),
    ("exclamation", sym::EXCLAMATION),
    ("exercise", sym::EXERCISE),
    ("exit_to_app", sym::EXIT_TO_APP),
    ("expand", sym::EXPAND),
    ("expand_all", sym::EXPAND_ALL),
    ("expand_circle_down", sym::EXPAND_CIRCLE_DOWN),
    ("expand_circle_right", sym::EXPAND_CIRCLE_RIGHT),
    ("expand_circle_up", sym::EXPAND_CIRCLE_UP),
    ("expand_content", sym::EXPAND_CONTENT),
    ("expand_less", sym::EXPAND_LESS),
    ("expand_more", sym::EXPAND_MORE),
    ("expansion_panels", sym::EXPANSION_PANELS),
    ("expension_panels", sym::EXPENSION_PANELS),
    ("experiment", sym::EXPERIMENT),
    ("explicit", sym::EXPLICIT),
    ("explore", sym::EXPLORE),
    ("explore_nearby", sym::EXPLORE_NEARBY),
    ("explore_off", sym::EXPLORE_OFF),
    ("explosion", sym::EXPLOSION),
    ("export_notes", sym::EXPORT_NOTES),
    ("exposure", sym::EXPOSURE),
    ("exposure_neg_1", sym::EXPOSURE_NEG_1),
    ("exposure_neg_2", sym::EXPOSURE_NEG_2),
    ("exposure_plus_1", sym::EXPOSURE_PLUS_1),
    ("exposure_plus_2", sym::EXPOSURE_PLUS_2),
    ("exposure_zero", sym::EXPOSURE_ZERO),
    ("extension", sym::EXTENSION),
    ("extension_off", sym::EXTENSION_OFF),
    ("eye_tracking", sym::EYE_TRACKING),
    ("eyebrow", sym::EYEBROW),
    ("eyeglasses", sym::EYEGLASSES),
    ("eyeglasses_2", sym::EYEGLASSES_2),
    ("eyeglasses_2_sound", sym::EYEGLASSES_2_SOUND),
    ("eyeglasses_3", sym::EYEGLASSES_3),
    ("face", sym::FACE),
    ("face_2", sym::FACE_2),
    ("face_3", sym::FACE_3),
    ("face_4", sym::FACE_4),
    ("face_5", sym::FACE_5),
    ("face_6", sym::FACE_6),
    ("face_down", sym::FACE_DOWN),
    ("face_left", sym::FACE_LEFT),
    ("face_nod", sym::FACE_NOD),
    ("face_retouching_natural", sym::FACE_RETOUCHING_NATURAL),
    ("face_retouching_off", sym::FACE_RETOUCHING_OFF),
    ("face_right", sym::FACE_RIGHT),
    ("face_shake", sym::FACE_SHAKE),
    ("face_unlock", sym::FACE_UNLOCK),
    ("face_up", sym::FACE_UP),
    ("fact_check", sym::FACT_CHECK),
    ("factory", sym::FACTORY),
    ("falling", sym::FALLING),
    ("familiar_face_and_zone", sym::FAMILIAR_FACE_AND_ZONE),
    ("family_group", sym::FAMILY_GROUP),
    ("family_history", sym::FAMILY_HISTORY),
    ("family_home", sym::FAMILY_HOME),
    ("family_link", sym::FAMILY_LINK),
    ("family_restroom", sym::FAMILY_RESTROOM),
    ("family_star", sym::FAMILY_STAR),
    ("fan_focus", sym::FAN_FOCUS),
    ("fan_indirect", sym::FAN_INDIRECT),
    ("farsight_digital", sym::FARSIGHT_DIGITAL),
    ("fast_forward", sym::FAST_FORWARD),
    ("fast_rewind", sym::FAST_REWIND),
    ("fastfood", sym::FASTFOOD),
    ("faucet", sym::FAUCET),
    ("favorite", sym::FAVORITE),
    ("favorite_border", sym::FAVORITE_BORDER),
    ("fax", sym::FAX),
    ("feature_search", sym::FEATURE_SEARCH),
    ("featured_play_list", sym::FEATURED_PLAY_LIST),
    ("featured_seasonal_and_gifts", sym::FEATURED_SEASONAL_AND_GIFTS),
    ("featured_video", sym::FEATURED_VIDEO),
    ("feed", sym::FEED),
    ("feedback", sym::FEEDBACK),
    ("female", sym::FEMALE),
    ("femur", sym::FEMUR),
    ("femur_alt", sym::FEMUR_ALT),
    ("fence", sym::FENCE),
    ("fertile", sym::FERTILE),
    ("festival", sym::FESTIVAL),
    ("fiber_dvr", sym::FIBER_DVR),
    ("fiber_manual_record", sym::FIBER_MANUAL_RECORD),
    ("fiber_new", sym::FIBER_NEW),
    ("fiber_pin", sym::FIBER_PIN),
    ("fiber_smart_record", sym::FIBER_SMART_RECORD),
    ("file_copy", sym::FILE_COPY),
    ("file_copy_off", sym::FILE_COPY_OFF),
    ("file_download", sym::FILE_DOWNLOAD),
    ("file_download_done", sym::FILE_DOWNLOAD_DONE),
    ("file_download_off", sym::FILE_DOWNLOAD_OFF),
    ("file_export", sym::FILE_EXPORT),
    ("file_json", sym::FILE_JSON),
    ("file_map", sym::FILE_MAP),
    ("file_map_stack", sym::FILE_MAP_STACK),
    ("file_open", sym::FILE_OPEN),
    ("file_png", sym::FILE_PNG),
    ("file_present", sym::FILE_PRESENT),
    ("file_save", sym::FILE_SAVE),
    ("file_save_off", sym::FILE_SAVE_OFF),
    ("file_upload", sym::FILE_UPLOAD),
    ("file_upload_off", sym::FILE_UPLOAD_OFF),
    ("files", sym::FILES),
    ("filter", sym::FILTER),
    ("filter_1", sym::FILTER_1),
    ("filter_2", sym::FILTER_2),
    ("filter_3", sym::FILTER_3),
    ("filter_4", sym::FILTER_4),
    ("filter_5", sym::FILTER_5),
    ("filter_6", sym::FILTER_6),
    ("filter_7", sym::FILTER_7),
    ("filter_8", sym::FILTER_8),
    ("filter_9", sym::FILTER_9),
    ("filter_9_plus", sym::FILTER_9_PLUS),
    ("filter_alt", sym::FILTER_ALT),
    ("filter_alt_off", sym::FILTER_ALT_OFF),
    ("filter_arrow_right", sym::FILTER_ARROW_RIGHT),
    ("filter_b_and_w", sym::FILTER_B_AND_W),
    ("filter_center_focus", sym::FILTER_CENTER_FOCUS),
    ("filter_drama", sym::FILTER_DRAMA),
    ("filter_frames", sym::FILTER_FRAMES),
    ("filter_hdr", sym::FILTER_HDR),
    ("filter_list", sym::FILTER_LIST),
    ("filter_list_alt", sym::FILTER_LIST_ALT),
    ("filter_list_off", sym::FILTER_LIST_OFF),
    ("filter_none", sym::FILTER_NONE),
    ("filter_retrolux", sym::FILTER_RETROLUX),
    ("filter_tilt_shift", sym::FILTER_TILT_SHIFT),
    ("filter_vintage", sym::FILTER_VINTAGE),
    ("finance", sym::FINANCE),
    ("finance_chip", sym::FINANCE_CHIP),
    ("finance_mode", sym::FINANCE_MODE),
    ("find_in_page", sym::FIND_IN_PAGE),
    ("find_replace", sym::FIND_REPLACE),
    ("fingerprint", sym::FINGERPRINT),
    ("fingerprint_off", sym::FINGERPRINT_OFF),
    ("fire_check", sym::FIRE_CHECK),
    ("fire_extinguisher", sym::FIRE_EXTINGUISHER),
    ("fire_hydrant", sym::FIRE_HYDRANT),
    ("fire_truck", sym::FIRE_TRUCK),
    ("fireplace", sym::FIREPLACE),
    ("first_page", sym::FIRST_PAGE),
    ("fit_page", sym::FIT_PAGE),
    ("fit_page_height", sym::FIT_PAGE_HEIGHT),
    ("fit_page_width", sym::FIT_PAGE_WIDTH),
    ("fit_screen", sym::FIT_SCREEN),
    ("fit_width", sym::FIT_WIDTH),
    ("fitness_center", sym::FITNESS_CENTER),
    ("fitness_tracker", sym::FITNESS_TRACKER),
    ("fitness_trackers", sym::FITNESS_TRACKERS),
    ("flag", sym::FLAG),
    ("flag_2", sym::FLAG_2),
    ("flag_check", sym::FLAG_CHECK),
    ("flag_circle", sym::FLAG_CIRCLE),
    ("flag_filled", sym::FLAG_FILLED),
    ("flaky", sym::FLAKY),
    ("flare", sym::FLARE),
    ("flash_auto", sym::FLASH_AUTO),
    ("flash_off", sym::FLASH_OFF),
    ("flash_on", sym::FLASH_ON),
    ("flashlight_off", sym::FLASHLIGHT_OFF),
    ("flashlight_on", sym::FLASHLIGHT_ON),
    ("flatware", sym::FLATWARE),
    ("flex_direction", sym::FLEX_DIRECTION),
    ("flex_no_wrap", sym::FLEX_NO_WRAP),
    ("flex_wrap", sym::FLEX_WRAP),
    ("flight", sym::FLIGHT),
    ("flight_class", sym::FLIGHT_CLASS),
    ("flight_land", sym::FLIGHT_LAND),
    ("flight_takeoff", sym::FLIGHT_TAKEOFF),
    ("flights_and_hotels", sym::FLIGHTS_AND_HOTELS),
    ("flightsmode", sym::FLIGHTSMODE),
    ("flip", sym::FLIP),
    ("flip_camera_android", sym::FLIP_CAMERA_ANDROID),
    ("flip_camera_ios", sym::FLIP_CAMERA_IOS),
    ("flip_to_back", sym::FLIP_TO_BACK),
    ("flip_to_front", sym::FLIP_TO_FRONT),
    ("float_landscape_2", sym::FLOAT_LANDSCAPE_2),
    ("float_portrait_2", sym::FLOAT_PORTRAIT_2),
    ("flood", sym::FLOOD),
    ("floor", sym::FLOOR),
    ("floor_lamp", sym::FLOOR_LAMP),
    ("flourescent", sym::FLOURESCENT),
    ("flowchart", sym::FLOWCHART),
    ("flowsheet", sym::FLOWSHEET),
    ("fluid", sym::FLUID),
    ("fluid_balance", sym::FLUID_BALANCE),
    ("fluid_med", sym::FLUID_MED),
    ("fluorescent", sym::FLUORESCENT),
    ("flutter", sym::FLUTTER),
    ("flutter_dash", sym::FLUTTER_DASH),
    ("flyover", sym::FLYOVER),
    ("fmd_bad", sym::FMD_BAD),
    ("fmd_good", sym::FMD_GOOD),
    ("foggy", sym::FOGGY),
    ("folded_hands", sym::FOLDED_HANDS),
    ("folder", sym::FOLDER),
    ("folder_check", sym::FOLDER_CHECK),
    ("folder_check_2", sym::FOLDER_CHECK_2),
    ("folder_code", sym::FOLDER_CODE),
    ("folder_copy", sym::FOLDER_COPY),
    ("folder_data", sym::FOLDER_DATA),
    ("folder_delete", sym::FOLDER_DELETE),
    ("folder_eye", sym::FOLDER_EYE),
    ("folder_info", sym::FOLDER_INFO),
    ("folder_limited", sym::FOLDER_LIMITED),
    ("folder_managed", sym::FOLDER_MANAGED),
    ("folder_match", sym::FOLDER_MATCH),
    ("folder_off", sym::FOLDER_OFF),
    ("folder_open", sym::FOLDER_OPEN),
    ("folder_shared", sym::FOLDER_SHARED),
    ("folder_special", sym::FOLDER_SPECIAL),
    ("folder_supervised", sym::FOLDER_SUPERVISED),
    ("folder_zip", sym::FOLDER_ZIP),
    ("follow_the_signs", sym::FOLLOW_THE_SIGNS),
    ("font_download", sym::FONT_DOWNLOAD),
    ("font_download_off", sym::FONT_DOWNLOAD_OFF),
    ("food_bank", sym::FOOD_BANK),
    ("foot_bones", sym::FOOT_BONES),
    ("footprint", sym::FOOTPRINT),
    ("for_you", sym::FOR_YOU),
    ("forest", sym::FOREST),
    ("fork_chart", sym::FORK_CHART),
    ("fork_left", sym::FORK_LEFT),
    ("fork_right", sym::FORK_RIGHT),
    ("fork_spoon", sym::FORK_SPOON),
    ("forklift", sym::FORKLIFT),
    ("format_align_center", sym::FORMAT_ALIGN_CENTER),
    ("format_align_justify", sym::FORMAT_ALIGN_JUSTIFY),
    ("format_align_left", sym::FORMAT_ALIGN_LEFT),
    ("format_align_right", sym::FORMAT_ALIGN_RIGHT),
    ("format_bold", sym::FORMAT_BOLD),
    ("format_clear", sym::FORMAT_CLEAR),
    ("format_color_fill", sym::FORMAT_COLOR_FILL),
    ("format_color_reset", sym::FORMAT_COLOR_RESET),
    ("format_color_text", sym::FORMAT_COLOR_TEXT),
    ("format_h1", sym::FORMAT_H1),
    ("format_h2", sym::FORMAT_H2),
    ("format_h3", sym::FORMAT_H3),
    ("format_h4", sym::FORMAT_H4),
    ("format_h5", sym::FORMAT_H5),
    ("format_h6", sym::FORMAT_H6),
    ("format_image_back", sym::FORMAT_IMAGE_BACK),
    ("format_image_break_left", sym::FORMAT_IMAGE_BREAK_LEFT),
    ("format_image_break_right", sym::FORMAT_IMAGE_BREAK_RIGHT),
    ("format_image_front", sym::FORMAT_IMAGE_FRONT),
    ("format_image_inline_left", sym::FORMAT_IMAGE_INLINE_LEFT),
    ("format_image_inline_right", sym::FORMAT_IMAGE_INLINE_RIGHT),
    ("format_image_left", sym::FORMAT_IMAGE_LEFT),
    ("format_image_right", sym::FORMAT_IMAGE_RIGHT),
    ("format_indent_decrease", sym::FORMAT_INDENT_DECREASE),
    ("format_indent_increase", sym::FORMAT_INDENT_INCREASE),
    ("format_ink_highlighter", sym::FORMAT_INK_HIGHLIGHTER),
    ("format_italic", sym::FORMAT_ITALIC),
    ("format_letter_spacing", sym::FORMAT_LETTER_SPACING),
    ("format_letter_spacing_2", sym::FORMAT_LETTER_SPACING_2),
    ("format_letter_spacing_standard", sym::FORMAT_LETTER_SPACING_STANDARD),
    ("format_letter_spacing_wide", sym::FORMAT_LETTER_SPACING_WIDE),
    ("format_letter_spacing_wider", sym::FORMAT_LETTER_SPACING_WIDER),
    ("format_line_spacing", sym::FORMAT_LINE_SPACING),
    ("format_list_bulleted", sym::FORMAT_LIST_BULLETED),
    ("format_list_bulleted_add", sym::FORMAT_LIST_BULLETED_ADD),
    ("format_list_numbered", sym::FORMAT_LIST_NUMBERED),
    ("format_list_numbered_rtl", sym::FORMAT_LIST_NUMBERED_RTL),
    ("format_overline", sym::FORMAT_OVERLINE),
    ("format_paint", sym::FORMAT_PAINT),
    ("format_paint_off", sym::FORMAT_PAINT_OFF),
    ("format_paragraph", sym::FORMAT_PARAGRAPH),
    ("format_quote", sym::FORMAT_QUOTE),
    ("format_quote_off", sym::FORMAT_QUOTE_OFF),
    ("format_shapes", sym::FORMAT_SHAPES),
    ("format_size", sym::FORMAT_SIZE),
    ("format_strikethrough", sym::FORMAT_STRIKETHROUGH),
    ("format_text_clip", sym::FORMAT_TEXT_CLIP),
    ("format_text_overflow", sym::FORMAT_TEXT_OVERFLOW),
    ("format_text_wrap", sym::FORMAT_TEXT_WRAP),
    ("format_textdirection_l_to_r", sym::FORMAT_TEXTDIRECTION_L_TO_R),
    ("format_textdirection_r_to_l", sym::FORMAT_TEXTDIRECTION_R_TO_L),
    ("format_textdirection_vertical", sym::FORMAT_TEXTDIRECTION_VERTICAL),
    ("format_underlined", sym::FORMAT_UNDERLINED),
    ("format_underlined_squiggle", sym::FORMAT_UNDERLINED_SQUIGGLE),
    ("forms_add_on", sym::FORMS_ADD_ON),
    ("forms_apps_script", sym::FORMS_APPS_SCRIPT),
    ("fort", sym::FORT),
    ("forum", sym::FORUM),
    ("forward", sym::FORWARD),
    ("forward_10", sym::FORWARD_10),
    ("forward_30", sym::FORWARD_30),
    ("forward_5", sym::FORWARD_5),
    ("forward_circle", sym::FORWARD_CIRCLE),
    ("forward_media", sym::FORWARD_MEDIA),
    ("forward_to_inbox", sym::FORWARD_TO_INBOX),
    ("foundation", sym::FOUNDATION),
    ("fragrance", sym::FRAGRANCE),
    ("frame_bug", sym::FRAME_BUG),
    ("frame_exclamation", sym::FRAME_EXCLAMATION),
    ("frame_inspect", sym::FRAME_INSPECT),
    ("frame_person", sym::FRAME_PERSON),
    ("frame_person_mic", sym::FRAME_PERSON_MIC),
    ("frame_person_off", sym::FRAME_PERSON_OFF),
    ("frame_reload", sym::FRAME_RELOAD),
    ("frame_source", sym::FRAME_SOURCE),
    ("free_breakfast", sym::FREE_BREAKFAST),
    ("free_cancellation", sym::FREE_CANCELLATION),
    ("front_hand", sym::FRONT_HAND),
    ("front_loader", sym::FRONT_LOADER),
    ("full_coverage", sym::FULL_COVERAGE),
    ("full_hd", sym::FULL_HD),
    ("full_stacked_bar_chart", sym::FULL_STACKED_BAR_CHART),
    ("fullscreen", sym::FULLSCREEN),
    ("fullscreen_exit", sym::FULLSCREEN_EXIT),
    ("fullscreen_portrait", sym::FULLSCREEN_PORTRAIT),
    ("function", sym::FUNCTION),
    ("functions", sym::FUNCTIONS),
    ("funicular", sym::FUNICULAR),
    ("g_mobiledata", sym::G_MOBILEDATA),
    ("g_mobiledata_badge", sym::G_MOBILEDATA_BADGE),
    ("g_translate", sym::G_TRANSLATE),
    ("gallery_thumbnail", sym::GALLERY_THUMBNAIL),
    ("game_bumper_left", sym::GAME_BUMPER_LEFT),
    ("game_bumper_right", sym::GAME_BUMPER_RIGHT),
    ("game_button_l", sym::GAME_BUTTON_L),
    ("game_button_l1", sym::GAME_BUTTON_L1),
    ("game_button_l2", sym::GAME_BUTTON_L2),
    ("game_button_r", sym::GAME_BUTTON_R),
    ("game_button_r1", sym::GAME_BUTTON_R1),
    ("game_button_r2", sym::GAME_BUTTON_R2),
    ("game_button_zl", sym::GAME_BUTTON_ZL),
    ("game_button_zr", sym::GAME_BUTTON_ZR),
    ("game_stick_l3", sym::GAME_STICK_L3),
    ("game_stick_left", sym::GAME_STICK_LEFT),
    ("game_stick_r3", sym::GAME_STICK_R3),
    ("game_stick_right", sym::GAME_STICK_RIGHT),
    ("game_trigger_left", sym::GAME_TRIGGER_LEFT),
    ("game_trigger_right", sym::GAME_TRIGGER_RIGHT),
    ("gamepad", sym::GAMEPAD),
    ("gamepad_circle_down", sym::GAMEPAD_CIRCLE_DOWN),
    ("gamepad_circle_left", sym::GAMEPAD_CIRCLE_LEFT),
    ("gamepad_circle_right", sym::GAMEPAD_CIRCLE_RIGHT),
    ("gamepad_circle_up", sym::GAMEPAD_CIRCLE_UP),
    ("gamepad_down", sym::GAMEPAD_DOWN),
    ("gamepad_left", sym::GAMEPAD_LEFT),
    ("gamepad_right", sym::GAMEPAD_RIGHT),
    ("gamepad_up", sym::GAMEPAD_UP),
    ("games", sym::GAMES),
    ("garage", sym::GARAGE),
    ("garage_check", sym::GARAGE_CHECK),
    ("garage_door", sym::GARAGE_DOOR),
    ("garage_door_open", sym::GARAGE_DOOR_OPEN),
    ("garage_home", sym::GARAGE_HOME),
    ("garage_money", sym::GARAGE_MONEY),
    ("garden_cart", sym::GARDEN_CART),
    ("gas_meter", sym::GAS_METER),
    ("gastroenterology", sym::GASTROENTEROLOGY),
    ("gate", sym::GATE),
    ("gavel", sym::GAVEL),
    ("general_device", sym::GENERAL_DEVICE),
    ("generating_tokens", sym::GENERATING_TOKENS),
    ("genetics", sym::GENETICS),
    ("genres", sym::GENRES),
    ("gesture", sym::GESTURE),
    ("gesture_select", sym::GESTURE_SELECT),
    ("get_app", sym::GET_APP),
    ("gif", sym::GIF),
    ("gif_2", sym::GIF_2),
    ("gif_box", sym::GIF_BOX),
    ("girl", sym::GIRL),
    ("gite", sym::GITE),
    ("glass_cup", sym::GLASS_CUP),
    ("globe", sym::GLOBE),
    ("globe_2_cancel", sym::GLOBE_2_CANCEL),
    ("globe_2_question", sym::GLOBE_2_QUESTION),
    ("globe_asia", sym::GLOBE_ASIA),
    ("globe_book", sym::GLOBE_BOOK),
    ("globe_clock", sym::GLOBE_CLOCK),
    ("globe_location_pin", sym::GLOBE_LOCATION_PIN),
    ("globe_uk", sym::GLOBE_UK),
    ("glucose", sym::GLUCOSE),
    ("glyphs", sym::GLYPHS),
    ("go_to_line", sym::GO_TO_LINE),
    ("golf_course", sym::GOLF_COURSE),
    ("gondola_lift", sym::GONDOLA_LIFT),
    ("google_home_devices", sym::GOOGLE_HOME_DEVICES),
    ("google_plus_reshare", sym::GOOGLE_PLUS_RESHARE),
    ("google_tv_remote", sym::GOOGLE_TV_REMOTE),
    ("google_wifi", sym::GOOGLE_WIFI),
    ("gpp_bad", sym::GPP_BAD),
    ("gpp_good", sym::GPP_GOOD),
    ("gpp_maybe", sym::GPP_MAYBE),
    ("gps_fixed", sym::GPS_FIXED),
    ("gps_not_fixed", sym::GPS_NOT_FIXED),
    ("gps_off", sym::GPS_OFF),
    ("grade", sym::GRADE),
    ("gradient", sym::GRADIENT),
    ("grading", sym::GRADING),
    ("grain", sym::GRAIN),
    ("graph_1", sym::GRAPH_1),
    ("graph_2", sym::GRAPH_2),
    ("graph_3", sym::GRAPH_3),
    ("graph_4", sym::GRAPH_4),
    ("graph_5", sym::GRAPH_5),
    ("graph_6", sym::GRAPH_6),
    ("graph_7", sym::GRAPH_7),
    ("graph_8", sym::GRAPH_8),
    ("graphic_eq", sym::GRAPHIC_EQ),
    ("graphic_eq_off", sym::GRAPHIC_EQ_OFF),
    ("grass", sym::GRASS),
    ("grid_3x3", sym::GRID_3X3),
    ("grid_3x3_off", sym::GRID_3X3_OFF),
    ("grid_4x4", sym::GRID_4X4),
    ("grid_goldenratio", sym::GRID_GOLDENRATIO),
    ("grid_guides", sym::GRID_GUIDES),
    ("grid_layout_side", sym::GRID_LAYOUT_SIDE),
    ("grid_off", sym::GRID_OFF),
    ("grid_on", sym::GRID_ON),
    ("grid_view", sym::GRID_VIEW),
    ("grocery", sym::GROCERY),
    ("group", sym::GROUP),
    ("group_add", sym::GROUP_ADD),
    ("group_off", sym::GROUP_OFF),
    ("group_remove", sym::GROUP_REMOVE),
    ("group_search", sym::GROUP_SEARCH),
    ("group_work", sym::GROUP_WORK),
    ("grouped_bar_chart", sym::GROUPED_BAR_CHART),
    ("groups", sym::GROUPS),
    ("groups_2", sym::GROUPS_2),
    ("groups_3", sym::GROUPS_3),
    ("guardian", sym::GUARDIAN),
    ("gynecology", sym::GYNECOLOGY),
    ("h_mobiledata", sym::H_MOBILEDATA),
    ("h_mobiledata_badge", sym::H_MOBILEDATA_BADGE),
    ("h_plus_mobiledata", sym::H_PLUS_MOBILEDATA),
    ("h_plus_mobiledata_badge", sym::H_PLUS_MOBILEDATA_BADGE),
    ("hail", sym::HAIL),
    ("hallway", sym::HALLWAY),
    ("hanami_dango", sym::HANAMI_DANGO),
    ("hand_bones", sym::HAND_BONES),
    ("hand_gesture", sym::HAND_GESTURE),
    ("hand_gesture_off", sym::HAND_GESTURE_OFF),
    ("hand_meal", sym::HAND_MEAL),
    ("hand_package", sym::HAND_PACKAGE),
    ("handheld_controller", sym::HANDHELD_CONTROLLER),
    ("handshake", sym::HANDSHAKE),
    ("handwriting_recognition", sym::HANDWRITING_RECOGNITION),
    ("handyman", sym::HANDYMAN),
    ("hangout_video", sym::HANGOUT_VIDEO),
    ("hangout_video_off", sym::HANGOUT_VIDEO_OFF),
    ("hard_disk", sym::HARD_DISK),
    ("hard_drive", sym::HARD_DRIVE),
    ("hard_drive_2", sym::HARD_DRIVE_2),
    ("hardware", sym::HARDWARE),
    ("hd", sym::HD),
    ("hdr_auto", sym::HDR_AUTO),
    ("hdr_auto_select", sym::HDR_AUTO_SELECT),
    ("hdr_enhanced_select", sym::HDR_ENHANCED_SELECT),
    ("hdr_off", sym::HDR_OFF),
    ("hdr_off_select", sym::HDR_OFF_SELECT),
    ("hdr_on", sym::HDR_ON),
    ("hdr_on_select", sym::HDR_ON_SELECT),
    ("hdr_plus", sym::HDR_PLUS),
    ("hdr_plus_off", sym::HDR_PLUS_OFF),
    ("hdr_strong", sym::HDR_STRONG),
    ("hdr_weak", sym::HDR_WEAK),
    ("head_mounted_device", sym::HEAD_MOUNTED_DEVICE),
    ("headphones", sym::HEADPHONES),
    ("headphones_battery", sym::HEADPHONES_BATTERY),
    ("headset", sym::HEADSET),
    ("headset_mic", sym::HEADSET_MIC),
    ("headset_off", sym::HEADSET_OFF),
    ("healing", sym::HEALING),
    ("health_and_beauty", sym::HEALTH_AND_BEAUTY),
    ("health_and_safety", sym::HEALTH_AND_SAFETY),
    ("health_cross", sym::HEALTH_CROSS),
    ("health_metrics", sym::HEALTH_METRICS),
    ("heap_snapshot_large", sym::HEAP_SNAPSHOT_LARGE),
    ("heap_snapshot_multiple", sym::HEAP_SNAPSHOT_MULTIPLE),
    ("heap_snapshot_thumbnail", sym::HEAP_SNAPSHOT_THUMBNAIL),
    ("hearing", sym::HEARING),
    ("hearing_aid", sym::HEARING_AID),
    ("hearing_aid_disabled", sym::HEARING_AID_DISABLED),
    ("hearing_aid_disabled_left", sym::HEARING_AID_DISABLED_LEFT),
    ("hearing_aid_left", sym::HEARING_AID_LEFT),
    ("hearing_disabled", sym::HEARING_DISABLED),
    ("heart_broken", sym::HEART_BROKEN),
    ("heart_check", sym::HEART_CHECK),
    ("heart_minus", sym::HEART_MINUS),
    ("heart_plus", sym::HEART_PLUS),
    ("heart_smile", sym::HEART_SMILE),
    ("heat", sym::HEAT),
    ("heat_pump", sym::HEAT_PUMP),
    ("heat_pump_balance", sym::HEAT_PUMP_BALANCE),
    ("height", sym::HEIGHT),
    ("helicopter", sym::HELICOPTER),
    ("help", sym::HELP),
    ("help_center", sym::HELP_CENTER),
    ("help_clinic", sym::HELP_CLINIC),
    ("help_outline", sym::HELP_OUTLINE),
    ("hematology", sym::HEMATOLOGY),
    ("hevc", sym::HEVC),
    ("hexagon", sym::HEXAGON),
    ("hide", sym::HIDE),
    ("hide_image", sym::HIDE_IMAGE),
    ("hide_source", sym::HIDE_SOURCE),
    ("high_chair", sym::HIGH_CHAIR),
    ("high_density", sym::HIGH_DENSITY),
    ("high_quality", sym::HIGH_QUALITY),
    ("high_quality_off", sym::HIGH_QUALITY_OFF),
    ("high_res", sym::HIGH_RES),
    ("highlight", sym::HIGHLIGHT),
    ("highlight_alt", sym::HIGHLIGHT_ALT),
    ("highlight_keyboard_focus", sym::HIGHLIGHT_KEYBOARD_FOCUS),
    ("highlight_mouse_cursor", sym::HIGHLIGHT_MOUSE_CURSOR),
    ("highlight_off", sym::HIGHLIGHT_OFF),
    ("highlight_text_cursor", sym::HIGHLIGHT_TEXT_CURSOR),
    ("highlighter_size_1", sym::HIGHLIGHTER_SIZE_1),
    ("highlighter_size_2", sym::HIGHLIGHTER_SIZE_2),
    ("highlighter_size_3", sym::HIGHLIGHTER_SIZE_3),
    ("highlighter_size_4", sym::HIGHLIGHTER_SIZE_4),
    ("highlighter_size_5", sym::HIGHLIGHTER_SIZE_5),
    ("hiking", sym::HIKING),
    ("history", sym::HISTORY),
    ("history_2", sym::HISTORY_2),
    ("history_edu", sym::HISTORY_EDU),
    ("history_off", sym::HISTORY_OFF),
    ("history_toggle_off", sym::HISTORY_TOGGLE_OFF),
    ("hive", sym::HIVE),
    ("hls", sym::HLS),
    ("hls_off", sym::HLS_OFF),
    ("holiday_village", sym::HOLIDAY_VILLAGE),
    ("home", sym::HOME),
    ("home_and_garden", sym::HOME_AND_GARDEN),
    ("home_app_logo", sym::HOME_APP_LOGO),
    ("home_filled", sym::HOME_FILLED),
    ("home_health", sym::HOME_HEALTH),
    ("home_improvement_and_tools", sym::HOME_IMPROVEMENT_AND_TOOLS),
    ("home_iot_device", sym::HOME_IOT_DEVICE),
    ("home_max", sym::HOME_MAX),
    ("home_max_dots", sym::HOME_MAX_DOTS),
    ("home_mini", sym::HOME_MINI),
    ("home_pin", sym::HOME_PIN),
    ("home_repair_service", sym::HOME_REPAIR_SERVICE),
    ("home_speaker", sym::HOME_SPEAKER),
    ("home_storage", sym::HOME_STORAGE),
    ("home_storage_gear", sym::HOME_STORAGE_GEAR),
    ("home_work", sym::HOME_WORK),
    ("horizontal_align_center", sym::HORIZONTAL_ALIGN_CENTER),
    ("horizontal_align_left", sym::HORIZONTAL_ALIGN_LEFT),
    ("horizontal_align_right", sym::HORIZONTAL_ALIGN_RIGHT),
    ("horizontal_distribute", sym::HORIZONTAL_DISTRIBUTE),
    ("horizontal_rule", sym::HORIZONTAL_RULE),
    ("horizontal_split", sym::HORIZONTAL_SPLIT),
    ("host", sym::HOST),
    ("hot_tub", sym::HOT_TUB),
    ("hotel", sym::HOTEL),
    ("hotel_class", sym::HOTEL_CLASS),
    ("hourglass", sym::HOURGLASS),
    ("hourglass_arrow_down", sym::HOURGLASS_ARROW_DOWN),
    ("hourglass_arrow_up", sym::HOURGLASS_ARROW_UP),
    ("hourglass_bottom", sym::HOURGLASS_BOTTOM),
    ("hourglass_check", sym::HOURGLASS_CHECK),
    ("hourglass_disabled", sym::HOURGLASS_DISABLED),
    ("hourglass_empty", sym::HOURGLASS_EMPTY),
    ("hourglass_full", sym::HOURGLASS_FULL),
    ("hourglass_pause", sym::HOURGLASS_PAUSE),
    ("hourglass_top", sym::HOURGLASS_TOP),
    ("house", sym::HOUSE),
    ("house_siding", sym::HOUSE_SIDING),
    ("house_with_shield", sym::HOUSE_WITH_SHIELD),
    ("houseboat", sym::HOUSEBOAT),
    ("household_supplies", sym::HOUSEHOLD_SUPPLIES),
    ("hov", sym::HOV),
    ("how_to_reg", sym::HOW_TO_REG),
    ("how_to_vote", sym::HOW_TO_VOTE),
    ("hr_resting", sym::HR_RESTING),
    ("html", sym::HTML),
    ("http", sym::HTTP),
    ("https", sym::HTTPS),
    ("hub", sym::HUB),
    ("humerus", sym::HUMERUS),
    ("humerus_alt", sym::HUMERUS_ALT),
    ("humidity_high", sym::HUMIDITY_HIGH),
    ("humidity_indoor", sym::HUMIDITY_INDOOR),
    ("humidity_low", sym::HUMIDITY_LOW),
    ("humidity_mid", sym::HUMIDITY_MID),
    ("humidity_percentage", sym::HUMIDITY_PERCENTAGE),
    ("hvac", sym::HVAC),
    ("hvac_max_defrost", sym::HVAC_MAX_DEFROST),
    ("ice_skating", sym::ICE_SKATING),
    ("icecream", sym::ICECREAM),
    ("id_card", sym::ID_CARD),
    ("id_card_2", sym::ID_CARD_2),
    ("identity_aware_proxy", sym::IDENTITY_AWARE_PROXY),
    ("identity_platform", sym::IDENTITY_PLATFORM),
    ("ifl", sym::IFL),
    ("iframe", sym::IFRAME),
    ("iframe_off", sym::IFRAME_OFF),
    ("image", sym::IMAGE),
    ("image_arrow_up", sym::IMAGE_ARROW_UP),
    ("image_aspect_ratio", sym::IMAGE_ASPECT_RATIO),
    ("image_inset", sym::IMAGE_INSET),
    ("image_not_supported", sym::IMAGE_NOT_SUPPORTED),
    ("image_search", sym::IMAGE_SEARCH),
    ("imagesearch_roller", sym::IMAGESEARCH_ROLLER),
    ("imagesmode", sym::IMAGESMODE),
    ("immunology", sym::IMMUNOLOGY),
    ("import_contacts", sym::IMPORT_CONTACTS),
    ("import_export", sym::IMPORT_EXPORT),
    ("important_devices", sym::IMPORTANT_DEVICES),
    ("in_home_mode", sym::IN_HOME_MODE),
    ("inactive_order", sym::INACTIVE_ORDER),
    ("inbox", sym::INBOX),
    ("inbox_customize", sym::INBOX_CUSTOMIZE),
    ("inbox_text", sym::INBOX_TEXT),
    ("inbox_text_asterisk", sym::INBOX_TEXT_ASTERISK),
    ("inbox_text_person", sym::INBOX_TEXT_PERSON),
    ("inbox_text_share", sym::INBOX_TEXT_SHARE),
    ("incomplete_circle", sym::INCOMPLETE_CIRCLE),
    ("indeterminate_check_box", sym::INDETERMINATE_CHECK_BOX),
    ("indeterminate_question_box", sym::INDETERMINATE_QUESTION_BOX),
    ("info", sym::INFO),
    ("info_i", sym::INFO_I),
    ("infrared", sym::INFRARED),
    ("ink_eraser", sym::INK_ERASER),
    ("ink_eraser_off", sym::INK_ERASER_OFF),
    ("ink_highlighter", sym::INK_HIGHLIGHTER),
    ("ink_highlighter_move", sym::INK_HIGHLIGHTER_MOVE),
    ("ink_highlighter_off", sym::INK_HIGHLIGHTER_OFF),
    ("ink_marker", sym::INK_MARKER),
    ("ink_pen", sym::INK_PEN),
    ("ink_selection", sym::INK_SELECTION),
    ("inpatient", sym::INPATIENT),
    ("input", sym::INPUT),
    ("input_circle", sym::INPUT_CIRCLE),
    ("insert_chart", sym::INSERT_CHART),
    ("insert_chart_filled", sym::INSERT_CHART_FILLED),
    ("insert_chart_outlined", sym::INSERT_CHART_OUTLINED),
    ("insert_comment", sym::INSERT_COMMENT),
    ("insert_drive_file", sym::INSERT_DRIVE_FILE),
    ("insert_emoticon", sym::INSERT_EMOTICON),
    ("insert_invitation", sym::INSERT_INVITATION),
    ("insert_link", sym::INSERT_LINK),
    ("insert_page_break", sym::INSERT_PAGE_BREAK),
    ("insert_photo", sym::INSERT_PHOTO),
    ("insert_text", sym::INSERT_TEXT),
    ("insights", sym::INSIGHTS),
    ("install_desktop", sym::INSTALL_DESKTOP),
    ("install_mobile", sym::INSTALL_MOBILE),
    ("instant_mix", sym::INSTANT_MIX),
    ("integration_instructions", sym::INTEGRATION_INSTRUCTIONS),
    ("interactive_space", sym::INTERACTIVE_SPACE),
    ("interests", sym::INTERESTS),
    ("interpreter_mode", sym::INTERPRETER_MODE),
    ("inventory", sym::INVENTORY),
    ("inventory_2", sym::INVENTORY_2),
    ("invert_colors", sym::INVERT_COLORS),
    ("invert_colors_off", sym::INVERT_COLORS_OFF),
    ("ios", sym::IOS),
    ("ios_share", sym::IOS_SHARE),
    ("iron", sym::IRON),
    ("iso", sym::ISO),
    ("jamboard_kiosk", sym::JAMBOARD_KIOSK),
    ("japanese_curry", sym::JAPANESE_CURRY),
    ("japanese_flag", sym::JAPANESE_FLAG),
    ("javascript", sym::JAVASCRIPT),
    ("jewelry", sym::JEWELRY),
    ("join", sym::JOIN),
    ("join_full", sym::JOIN_FULL),
    ("join_inner", sym::JOIN_INNER),
    ("join_left", sym::JOIN_LEFT),
    ("join_right", sym::JOIN_RIGHT),
    ("joystick", sym::JOYSTICK),
    ("jump_to_element", sym::JUMP_TO_ELEMENT),
    ("kanji_alcohol", sym::KANJI_ALCOHOL),
    ("kayaking", sym::KAYAKING),
    ("kebab_dining", sym::KEBAB_DINING),
    ("keep", sym::KEEP),
    ("keep_off", sym::KEEP_OFF),
    ("keep_pin", sym::KEEP_PIN),
    ("keep_public", sym::KEEP_PUBLIC),
    ("kettle", sym::KETTLE),
    ("key", sym::KEY),
    ("key_off", sym::KEY_OFF),
    ("key_vertical", sym::KEY_VERTICAL),
    ("key_visualizer", sym::KEY_VISUALIZER),
    ("keyboard", sym::KEYBOARD),
    ("keyboard_alt", sym::KEYBOARD_ALT),
    ("keyboard_arrow_down", sym::KEYBOARD_ARROW_DOWN),
    ("keyboard_arrow_left", sym::KEYBOARD_ARROW_LEFT),
    ("keyboard_arrow_right", sym::KEYBOARD_ARROW_RIGHT),
    ("keyboard_arrow_up", sym::KEYBOARD_ARROW_UP),
    ("keyboard_backspace", sym::KEYBOARD_BACKSPACE),
    ("keyboard_capslock", sym::KEYBOARD_CAPSLOCK),
    ("keyboard_capslock_badge", sym::KEYBOARD_CAPSLOCK_BADGE),
    ("keyboard_command_key", sym::KEYBOARD_COMMAND_KEY),
    ("keyboard_control_key", sym::KEYBOARD_CONTROL_KEY),
    ("keyboard_double_arrow_down", sym::KEYBOARD_DOUBLE_ARROW_DOWN),
    ("keyboard_double_arrow_left", sym::KEYBOARD_DOUBLE_ARROW_LEFT),
    ("keyboard_double_arrow_right", sym::KEYBOARD_DOUBLE_ARROW_RIGHT),
    ("keyboard_double_arrow_up", sym::KEYBOARD_DOUBLE_ARROW_UP),
    ("keyboard_external_input", sym::KEYBOARD_EXTERNAL_INPUT),
    ("keyboard_full", sym::KEYBOARD_FULL),
    ("keyboard_hide", sym::KEYBOARD_HIDE),
    ("keyboard_keys", sym::KEYBOARD_KEYS),
    ("keyboard_lock", sym::KEYBOARD_LOCK),
    ("keyboard_lock_off", sym::KEYBOARD_LOCK_OFF),
    ("keyboard_off", sym::KEYBOARD_OFF),
    ("keyboard_onscreen", sym::KEYBOARD_ONSCREEN),
    ("keyboard_option_key", sym::KEYBOARD_OPTION_KEY),
    ("keyboard_previous_language", sym::KEYBOARD_PREVIOUS_LANGUAGE),
    ("keyboard_return", sym::KEYBOARD_RETURN),
    ("keyboard_tab", sym::KEYBOARD_TAB),
    ("keyboard_tab_rtl", sym::KEYBOARD_TAB_RTL),
    ("keyboard_voice", sym::KEYBOARD_VOICE),
    ("kid_star", sym::KID_STAR),
    ("king_bed", sym::KING_BED),
    ("kitchen", sym::KITCHEN),
    ("kitesurfing", sym::KITESURFING),
    ("lab_panel", sym::LAB_PANEL),
    ("lab_profile", sym::LAB_PROFILE),
    ("lab_research", sym::LAB_RESEARCH),
    ("label", sym::LABEL),
    ("label_important", sym::LABEL_IMPORTANT),
    ("label_important_outline", sym::LABEL_IMPORTANT_OUTLINE),
    ("label_off", sym::LABEL_OFF),
    ("label_outline", sym::LABEL_OUTLINE),
    ("labs", sym::LABS),
    ("lan", sym::LAN),
    ("landscape", sym::LANDSCAPE),
    ("landscape_2", sym::LANDSCAPE_2),
    ("landscape_2_edit", sym::LANDSCAPE_2_EDIT),
    ("landscape_2_off", sym::LANDSCAPE_2_OFF),
    ("landslide", sym::LANDSLIDE),
    ("language", sym::LANGUAGE),
    ("language_chinese_array", sym::LANGUAGE_CHINESE_ARRAY),
    ("language_chinese_cangjie", sym::LANGUAGE_CHINESE_CANGJIE),
    ("language_chinese_dayi", sym::LANGUAGE_CHINESE_DAYI),
    ("language_chinese_pinyin", sym::LANGUAGE_CHINESE_PINYIN),
    ("language_chinese_quick", sym::LANGUAGE_CHINESE_QUICK),
    ("language_chinese_wubi", sym::LANGUAGE_CHINESE_WUBI),
    ("language_french", sym::LANGUAGE_FRENCH),
    ("language_gb_english", sym::LANGUAGE_GB_ENGLISH),
    ("language_international", sym::LANGUAGE_INTERNATIONAL),
    ("language_japanese_kana", sym::LANGUAGE_JAPANESE_KANA),
    ("language_korean_latin", sym::LANGUAGE_KOREAN_LATIN),
    ("language_pinyin", sym::LANGUAGE_PINYIN),
    ("language_spanish", sym::LANGUAGE_SPANISH),
    ("language_us", sym::LANGUAGE_US),
    ("language_us_colemak", sym::LANGUAGE_US_COLEMAK),
    ("language_us_dvorak", sym::LANGUAGE_US_DVORAK),
    ("laps", sym::LAPS),
    ("laptop", sym::LAPTOP),
    ("laptop_car", sym::LAPTOP_CAR),
    ("laptop_chromebook", sym::LAPTOP_CHROMEBOOK),
    ("laptop_mac", sym::LAPTOP_MAC),
    ("laptop_windows", sym::LAPTOP_WINDOWS),
    ("lasso_select", sym::LASSO_SELECT),
    ("last_page", sym::LAST_PAGE),
    ("launch", sym::LAUNCH),
    ("laundry", sym::LAUNDRY),
    ("layers", sym::LAYERS),
    ("layers_clear", sym::LAYERS_CLEAR),
    ("lda", sym::LDA),
    ("leaderboard", sym::LEADERBOARD),
    ("leak_add", sym::LEAK_ADD),
    ("leak_remove", sym::LEAK_REMOVE),
    ("left_click", sym::LEFT_CLICK),
    ("left_panel_close", sym::LEFT_PANEL_CLOSE),
    ("left_panel_open", sym::LEFT_PANEL_OPEN),
    ("legend_toggle", sym::LEGEND_TOGGLE),
    ("lens", sym::LENS),
    ("lens_blur", sym::LENS_BLUR),
    ("letter_switch", sym::LETTER_SWITCH),
    ("library_add", sym::LIBRARY_ADD),
    ("library_add_check", sym::LIBRARY_ADD_CHECK),
    ("library_books", sym::LIBRARY_BOOKS),
    ("library_music", sym::LIBRARY_MUSIC),
    ("license", sym::LICENSE),
    ("lift_to_talk", sym::LIFT_TO_TALK),
    ("light", sym::LIGHT),
    ("light_group", sym::LIGHT_GROUP),
    ("light_group_2", sym::LIGHT_GROUP_2),
    ("light_mode", sym::LIGHT_MODE),
    ("light_mode_auto", sym::LIGHT_MODE_AUTO),
    ("light_off", sym::LIGHT_OFF),
    ("lightbulb", sym::LIGHTBULB),
    ("lightbulb_2", sym::LIGHTBULB_2),
    ("lightbulb_circle", sym::LIGHTBULB_CIRCLE),
    ("lightbulb_outline", sym::LIGHTBULB_OUTLINE),
    ("lightning_stand", sym::LIGHTNING_STAND),
    ("lightstrip", sym::LIGHTSTRIP),
    ("line_axis", sym::LINE_AXIS),
    ("line_curve", sym::LINE_CURVE),
    ("line_end", sym::LINE_END),
    ("line_end_arrow", sym::LINE_END_ARROW),
    ("line_end_arrow_notch", sym::LINE_END_ARROW_NOTCH),
    ("line_end_circle", sym::LINE_END_CIRCLE),
    ("line_end_diamond", sym::LINE_END_DIAMOND),
    ("line_end_square", sym::LINE_END_SQUARE),
    ("line_start", sym::LINE_START),
    ("line_start_arrow", sym::LINE_START_ARROW),
    ("line_start_arrow_notch", sym::LINE_START_ARROW_NOTCH),
    ("line_start_circle", sym::LINE_START_CIRCLE),
    ("line_start_diamond", sym::LINE_START_DIAMOND),
    ("line_start_square", sym::LINE_START_SQUARE),
    ("line_style", sym::LINE_STYLE),
    ("line_weight", sym::LINE_WEIGHT),
    ("linear_scale", sym::LINEAR_SCALE),
    ("link", sym::LINK),
    ("link_2", sym::LINK_2),
    ("link_off", sym::LINK_OFF),
    ("linked_camera", sym::LINKED_CAMERA),
    ("linked_services", sym::LINKED_SERVICES),
    ("lips", sym::LIPS),
    ("liquor", sym::LIQUOR),
    ("list", sym::LIST),
    ("list_2", sym::LIST_2),
    ("list_alt", sym::LIST_ALT),
    ("list_alt_add", sym::LIST_ALT_ADD),
    ("list_alt_check", sym::LIST_ALT_CHECK),
    ("list_arrow", sym::LIST_ARROW),
    ("lists", sym::LISTS),
    ("live_help", sym::LIVE_HELP),
    ("live_tv", sym::LIVE_TV),
    ("living", sym::LIVING),
    ("local_activity", sym::LOCAL_ACTIVITY),
    ("local_airport", sym::LOCAL_AIRPORT),
    ("local_atm", sym::LOCAL_ATM),
    ("local_bar", sym::LOCAL_BAR),
    ("local_cafe", sym::LOCAL_CAFE),
    ("local_car_wash", sym::LOCAL_CAR_WASH),
    ("local_convenience_store", sym::LOCAL_CONVENIENCE_STORE),
    ("local_dining", sym::LOCAL_DINING),
    ("local_drink", sym::LOCAL_DRINK),
    ("local_fire_department", sym::LOCAL_FIRE_DEPARTMENT),
    ("local_florist", sym::LOCAL_FLORIST),
    ("local_gas_station", sym::LOCAL_GAS_STATION),
    ("local_grocery_store", sym::LOCAL_GROCERY_STORE),
    ("local_hospital", sym::LOCAL_HOSPITAL),
    ("local_hotel", sym::LOCAL_HOTEL),
    ("local_laundry_service", sym::LOCAL_LAUNDRY_SERVICE),
    ("local_library", sym::LOCAL_LIBRARY),
    ("local_mall", sym::LOCAL_MALL),
    ("local_movies", sym::LOCAL_MOVIES),
    ("local_offer", sym::LOCAL_OFFER),
    ("local_parking", sym::LOCAL_PARKING),
    ("local_pharmacy", sym::LOCAL_PHARMACY),
    ("local_phone", sym::LOCAL_PHONE),
    ("local_pizza", sym::LOCAL_PIZZA),
    ("local_play", sym::LOCAL_PLAY),
    ("local_police", sym::LOCAL_POLICE),
    ("local_post_office", sym::LOCAL_POST_OFFICE),
    ("local_printshop", sym::LOCAL_PRINTSHOP),
    ("local_see", sym::LOCAL_SEE),
    ("local_shipping", sym::LOCAL_SHIPPING),
    ("local_taxi", sym::LOCAL_TAXI),
    ("location_automation", sym::LOCATION_AUTOMATION),
    ("location_away", sym::LOCATION_AWAY),
    ("location_chip", sym::LOCATION_CHIP),
    ("location_city", sym::LOCATION_CITY),
    ("location_disabled", sym::LOCATION_DISABLED),
    ("location_home", sym::LOCATION_HOME),
    ("location_off", sym::LOCATION_OFF),
    ("location_on", sym::LOCATION_ON),
    ("location_pin", sym::LOCATION_PIN),
    ("location_searching", sym::LOCATION_SEARCHING),
    ("locator_tag", sym::LOCATOR_TAG),
    ("lock", sym::LOCK),
    ("lock_clock", sym::LOCK_CLOCK),
    ("lock_open", sym::LOCK_OPEN),
    ("lock_open_circle", sym::LOCK_OPEN_CIRCLE),
    ("lock_open_right", sym::LOCK_OPEN_RIGHT),
    ("lock_outline", sym::LOCK_OUTLINE),
    ("lock_person", sym::LOCK_PERSON),
    ("lock_reset", sym::LOCK_RESET),
    ("login", sym::LOGIN),
    ("logo_dev", sym::LOGO_DEV),
    ("logout", sym::LOGOUT),
    ("looks", sym::LOOKS),
    ("looks_3", sym::LOOKS_3),
    ("looks_4", sym::LOOKS_4),
    ("looks_5", sym::LOOKS_5),
    ("looks_6", sym::LOOKS_6),
    ("looks_one", sym::LOOKS_ONE),
    ("looks_two", sym::LOOKS_TWO),
    ("loop", sym::LOOP),
    ("loupe", sym::LOUPE),
    ("low_density", sym::LOW_DENSITY),
    ("low_priority", sym::LOW_PRIORITY),
    ("lowercase", sym::LOWERCASE),
    ("loyalty", sym::LOYALTY),
    ("lte_mobiledata", sym::LTE_MOBILEDATA),
    ("lte_mobiledata_badge", sym::LTE_MOBILEDATA_BADGE),
    ("lte_plus_mobiledata", sym::LTE_PLUS_MOBILEDATA),
    ("lte_plus_mobiledata_badge", sym::LTE_PLUS_MOBILEDATA_BADGE),
    ("luggage", sym::LUGGAGE),
    ("lunch_dining", sym::LUNCH_DINING),
    ("lyrics", sym::LYRICS),
    ("macro_auto", sym::MACRO_AUTO),
    ("macro_off", sym::MACRO_OFF),
    ("magic_button", sym::MAGIC_BUTTON),
    ("magic_exchange", sym::MAGIC_EXCHANGE),
    ("magic_tether", sym::MAGIC_TETHER),
    ("magnification_large", sym::MAGNIFICATION_LARGE),
    ("magnification_small", sym::MAGNIFICATION_SMALL),
    ("magnify_docked", sym::MAGNIFY_DOCKED),
    ("magnify_fullscreen", sym::MAGNIFY_FULLSCREEN),
    ("mail", sym::MAIL),
    ("mail_asterisk", sym::MAIL_ASTERISK),
    ("mail_lock", sym::MAIL_LOCK),
    ("mail_off", sym::MAIL_OFF),
    ("mail_outline", sym::MAIL_OUTLINE),
    ("mail_shield", sym::MAIL_SHIELD),
    ("male", sym::MALE),
    ("man", sym::MAN),
    ("man_2", sym::MAN_2),
    ("man_3", sym::MAN_3),
    ("man_4", sym::MAN_4),
    ("manage_accounts", sym::MANAGE_ACCOUNTS),
    ("manage_history", sym::MANAGE_HISTORY),
    ("manage_search", sym::MANAGE_SEARCH),
    ("manga", sym::MANGA),
    ("manufacturing", sym::MANUFACTURING),
    ("map", sym::MAP),
    ("map_pin_heart", sym::MAP_PIN_HEART),
    ("map_pin_review", sym::MAP_PIN_REVIEW),
    ("map_search", sym::MAP_SEARCH),
    ("maps_home_work", sym::MAPS_HOME_WORK),
    ("maps_ugc", sym::MAPS_UGC),
    ("margin", sym::MARGIN),
    ("mark_as_unread", sym::MARK_AS_UNREAD),
    ("mark_chat_read", sym::MARK_CHAT_READ),
    ("mark_chat_unread", sym::MARK_CHAT_UNREAD),
    ("mark_email_read", sym::MARK_EMAIL_READ),
    ("mark_email_unread", sym::MARK_EMAIL_UNREAD),
    ("mark_unread_chat_alt", sym::MARK_UNREAD_CHAT_ALT),
    ("markdown", sym::MARKDOWN),
    ("markdown_copy", sym::MARKDOWN_COPY),
    ("markdown_paste", sym::MARKDOWN_PASTE),
    ("markunread", sym::MARKUNREAD),
    ("markunread_mailbox", sym::MARKUNREAD_MAILBOX),
    ("masked_transitions", sym::MASKED_TRANSITIONS),
    ("masked_transitions_add", sym::MASKED_TRANSITIONS_ADD),
    ("masks", sym::MASKS),
    ("massage", sym::MASSAGE),
    ("match_case", sym::MATCH_CASE),
    ("match_case_off", sym::MATCH_CASE_OFF),
    ("match_word", sym::MATCH_WORD),
    ("matter", sym::MATTER),
    ("maximize", sym::MAXIMIZE),
    ("meal_dinner", sym::MEAL_DINNER),
    ("meal_lunch", sym::MEAL_LUNCH),
    ("measuring_tape", sym::MEASURING_TAPE),
    ("media_bluetooth_off", sym::MEDIA_BLUETOOTH_OFF),
    ("media_bluetooth_on", sym::MEDIA_BLUETOOTH_ON),
    ("media_link", sym::MEDIA_LINK),
    ("media_output", sym::MEDIA_OUTPUT),
    ("media_output_off", sym::MEDIA_OUTPUT_OFF),
    ("mediation", sym::MEDIATION),
    ("medical_information", sym::MEDICAL_INFORMATION),
    ("medical_mask", sym::MEDICAL_MASK),
    ("medical_services", sym::MEDICAL_SERVICES),
    ("medication", sym::MEDICATION),
    ("medication_liquid", sym::MEDICATION_LIQUID),
    ("meeting_room", sym::MEETING_ROOM),
    ("memory", sym::MEMORY),
    ("memory_alt", sym::MEMORY_ALT),
    ("menstrual_health", sym::MENSTRUAL_HEALTH),
    ("menu", sym::MENU),
    ("menu_book", sym::MENU_BOOK),
    ("menu_book_2", sym::MENU_BOOK_2),
    ("menu_open", sym::MENU_OPEN),
    ("merge", sym::MERGE),
    ("merge_type", sym::MERGE_TYPE),
    ("message", sym::MESSAGE),
    ("metabolism", sym::METABOLISM),
    ("metro", sym::METRO),
    ("mfg_nest_yale_lock", sym::MFG_NEST_YALE_LOCK),
    ("mic", sym::MIC),
    ("mic_alert", sym::MIC_ALERT),
    ("mic_double", sym::MIC_DOUBLE),
    ("mic_external_off", sym::MIC_EXTERNAL_OFF),
    ("mic_external_on", sym::MIC_EXTERNAL_ON),
    ("mic_gear", sym::MIC_GEAR),
    ("mic_none", sym::MIC_NONE),
    ("mic_off", sym::MIC_OFF),
    ("microbiology", sym::MICROBIOLOGY),
    ("microwave", sym::MICROWAVE),
    ("microwave_gen", sym::MICROWAVE_GEN),
    ("military_tech", sym::MILITARY_TECH),
    ("mimo", sym::MIMO),
    ("mimo_disconnect", sym::MIMO_DISCONNECT),
    ("mindfulness", sym::MINDFULNESS),
    ("minimize", sym::MINIMIZE),
    ("minor_crash", sym::MINOR_CRASH),
    ("mintmark", sym::MINTMARK),
    ("missed_video_call", sym::MISSED_VIDEO_CALL),
    ("missed_video_call_filled", sym::MISSED_VIDEO_CALL_FILLED),
    ("missing_controller", sym::MISSING_CONTROLLER),
    ("mist", sym::MIST),
    ("mitre", sym::MITRE),
    ("mixture_med", sym::MIXTURE_MED),
    ("mms", sym::MMS),
    ("mobile", sym::MOBILE),
    ("mobile_2", sym::MOBILE_2),
    ("mobile_3", sym::MOBILE_3),
    ("mobile_alert", sym::MOBILE_ALERT),
    ("mobile_arrow_down", sym::MOBILE_ARROW_DOWN),
    ("mobile_arrow_right", sym::MOBILE_ARROW_RIGHT),
    ("mobile_arrow_up_right", sym::MOBILE_ARROW_UP_RIGHT),
    ("mobile_block", sym::MOBILE_BLOCK),
    ("mobile_camera", sym::MOBILE_CAMERA),
    ("mobile_camera_front", sym::MOBILE_CAMERA_FRONT),
    ("mobile_camera_rear", sym::MOBILE_CAMERA_REAR),
    ("mobile_cancel", sym::MOBILE_CANCEL),
    ("mobile_cast", sym::MOBILE_CAST),
    ("mobile_charge", sym::MOBILE_CHARGE),
    ("mobile_chat", sym::MOBILE_CHAT),
    ("mobile_check", sym::MOBILE_CHECK),
    ("mobile_code", sym::MOBILE_CODE),
    ("mobile_dock", sym::MOBILE_DOCK),
    ("mobile_dots", sym::MOBILE_DOTS),
    ("mobile_friendly", sym::MOBILE_FRIENDLY),
    ("mobile_gear", sym::MOBILE_GEAR),
    ("mobile_hand", sym::MOBILE_HAND),
    ("mobile_hand_left", sym::MOBILE_HAND_LEFT),
    ("mobile_hand_left_off", sym::MOBILE_HAND_LEFT_OFF),
    ("mobile_hand_off", sym::MOBILE_HAND_OFF),
    ("mobile_info", sym::MOBILE_INFO),
    ("mobile_landscape", sym::MOBILE_LANDSCAPE),
    ("mobile_layout", sym::MOBILE_LAYOUT),
    ("mobile_lock_landscape", sym::MOBILE_LOCK_LANDSCAPE),
    ("mobile_lock_portrait", sym::MOBILE_LOCK_PORTRAIT),
    ("mobile_loupe", sym::MOBILE_LOUPE),
    ("mobile_menu", sym::MOBILE_MENU),
    ("mobile_off", sym::MOBILE_OFF),
    ("mobile_question", sym::MOBILE_QUESTION),
    ("mobile_rotate", sym::MOBILE_ROTATE),
    ("mobile_rotate_lock", sym::MOBILE_ROTATE_LOCK),
    ("mobile_screen_share", sym::MOBILE_SCREEN_SHARE),
    ("mobile_screensaver", sym::MOBILE_SCREENSAVER),
    ("mobile_sensor_hi", sym::MOBILE_SENSOR_HI),
    ("mobile_sensor_lo", sym::MOBILE_SENSOR_LO),
    ("mobile_share", sym::MOBILE_SHARE),
    ("mobile_share_stack", sym::MOBILE_SHARE_STACK),
    ("mobile_sound", sym::MOBILE_SOUND),
    ("mobile_sound_2", sym::MOBILE_SOUND_2),
    ("mobile_sound_off", sym::MOBILE_SOUND_OFF),
    ("mobile_speaker", sym::MOBILE_SPEAKER),
    ("mobile_tap", sym::MOBILE_TAP),
    ("mobile_text", sym::MOBILE_TEXT),
    ("mobile_text_2", sym::MOBILE_TEXT_2),
    ("mobile_theft", sym::MOBILE_THEFT),
    ("mobile_ticket", sym::MOBILE_TICKET),
    ("mobile_unlock", sym::MOBILE_UNLOCK),
    ("mobile_vibrate", sym::MOBILE_VIBRATE),
    ("mobile_wrench", sym::MOBILE_WRENCH),
    ("mobiledata_arrows", sym::MOBILEDATA_ARROWS),
    ("mobiledata_off", sym::MOBILEDATA_OFF),
    ("mode", sym::MODE),
    ("mode_comment", sym::MODE_COMMENT),
    ("mode_cool", sym::MODE_COOL),
    ("mode_cool_off", sym::MODE_COOL_OFF),
    ("mode_dual", sym::MODE_DUAL),
    ("mode_edit", sym::MODE_EDIT),
    ("mode_edit_outline", sym::MODE_EDIT_OUTLINE),
    ("mode_fan", sym::MODE_FAN),
    ("mode_fan_2", sym::MODE_FAN_2),
    ("mode_fan_off", sym::MODE_FAN_OFF),
    ("mode_heat", sym::MODE_HEAT),
    ("mode_heat_cool", sym::MODE_HEAT_COOL),
    ("mode_heat_off", sym::MODE_HEAT_OFF),
    ("mode_night", sym::MODE_NIGHT),
    ("mode_of_travel", sym::MODE_OF_TRAVEL),
    ("mode_off_on", sym::MODE_OFF_ON),
    ("mode_standby", sym::MODE_STANDBY),
    ("model_training", sym::MODEL_TRAINING),
    ("modeling", sym::MODELING),
    ("monetization_on", sym::MONETIZATION_ON),
    ("money", sym::MONEY),
    ("money_bag", sym::MONEY_BAG),
    ("money_off", sym::MONEY_OFF),
    ("money_off_csred", sym::MONEY_OFF_CSRED),
    ("money_range", sym::MONEY_RANGE),
    ("monitor", sym::MONITOR),
    ("monitor_heart", sym::MONITOR_HEART),
    ("monitor_weight", sym::MONITOR_WEIGHT),
    ("monitor_weight_gain", sym::MONITOR_WEIGHT_GAIN),
    ("monitor_weight_loss", sym::MONITOR_WEIGHT_LOSS),
    ("monitoring", sym::MONITORING),
    ("monochrome_photos", sym::MONOCHROME_PHOTOS),
    ("monorail", sym::MONORAIL),
    ("mood", sym::MOOD),
    ("mood_bad", sym::MOOD_BAD),
    ("mood_heart", sym::MOOD_HEART),
    ("moon_stars", sym::MOON_STARS),
    ("mop", sym::MOP),
    ("moped", sym::MOPED),
    ("moped_package", sym::MOPED_PACKAGE),
    ("more", sym::MORE),
    ("more_down", sym::MORE_DOWN),
    ("more_horiz", sym::MORE_HORIZ),
    ("more_time", sym::MORE_TIME),
    ("more_up", sym::MORE_UP),
    ("more_vert", sym::MORE_VERT),
    ("mosque", sym::MOSQUE),
    ("motion_blur", sym::MOTION_BLUR),
    ("motion_mode", sym::MOTION_MODE),
    ("motion_photos_auto", sym::MOTION_PHOTOS_AUTO),
    ("motion_photos_off", sym::MOTION_PHOTOS_OFF),
    ("motion_photos_on", sym::MOTION_PHOTOS_ON),
    ("motion_photos_pause", sym::MOTION_PHOTOS_PAUSE),
    ("motion_photos_paused", sym::MOTION_PHOTOS_PAUSED),
    ("motion_play", sym::MOTION_PLAY),
    ("motion_sensor_active", sym::MOTION_SENSOR_ACTIVE),
    ("motion_sensor_alert", sym::MOTION_SENSOR_ALERT),
    ("motion_sensor_idle", sym::MOTION_SENSOR_IDLE),
    ("motion_sensor_urgent", sym::MOTION_SENSOR_URGENT),
    ("motorcycle", sym::MOTORCYCLE),
    ("mountain_flag", sym::MOUNTAIN_FLAG),
    ("mountain_steam", sym::MOUNTAIN_STEAM),
    ("mouse", sym::MOUSE),
    ("mouse_lock", sym::MOUSE_LOCK),
    ("mouse_lock_off", sym::MOUSE_LOCK_OFF),
    ("move", sym::MOVE),
    ("move_down", sym::MOVE_DOWN),
    ("move_group", sym::MOVE_GROUP),
    ("move_item", sym::MOVE_ITEM),
    ("move_location", sym::MOVE_LOCATION),
    ("move_selection_down", sym::MOVE_SELECTION_DOWN),
    ("move_selection_left", sym::MOVE_SELECTION_LEFT),
    ("move_selection_right", sym::MOVE_SELECTION_RIGHT),
    ("move_selection_up", sym::MOVE_SELECTION_UP),
    ("move_to_inbox", sym::MOVE_TO_INBOX),
    ("move_up", sym::MOVE_UP),
    ("moved_location", sym::MOVED_LOCATION),
    ("movie", sym::MOVIE),
    ("movie_creation", sym::MOVIE_CREATION),
    ("movie_edit", sym::MOVIE_EDIT),
    ("movie_edit_off", sym::MOVIE_EDIT_OFF),
    ("movie_filter", sym::MOVIE_FILTER),
    ("movie_info", sym::MOVIE_INFO),
    ("movie_off", sym::MOVIE_OFF),
    ("movie_speaker", sym::MOVIE_SPEAKER),
    ("moving", sym::MOVING),
    ("moving_beds", sym::MOVING_BEDS),
    ("moving_ministry", sym::MOVING_MINISTRY),
    ("mp", sym::MP),
    ("multicooker", sym::MULTICOOKER),
    ("multiline_chart", sym::MULTILINE_CHART),
    ("multimodal_hand_eye", sym::MULTIMODAL_HAND_EYE),
    ("multiple_airports", sym::MULTIPLE_AIRPORTS),
    ("multiple_stop", sym::MULTIPLE_STOP),
    ("museum", sym::MUSEUM),
    ("music_cast", sym::MUSIC_CAST),
    ("music_history", sym::MUSIC_HISTORY),
    ("music_note", sym::MUSIC_NOTE),
    ("music_note_2", sym::MUSIC_NOTE_2),
    ("music_note_add", sym::MUSIC_NOTE_ADD),
    ("music_off", sym::MUSIC_OFF),
    ("music_video", sym::MUSIC_VIDEO),
    ("my_location", sym::MY_LOCATION),
    ("mystery", sym::MYSTERY),
    ("nat", sym::NAT),
    ("nature", sym::NATURE),
    ("nature_people", sym::NATURE_PEOPLE),
    ("navigate_before", sym::NAVIGATE_BEFORE),
    ("navigate_next", sym::NAVIGATE_NEXT),
    ("navigation", sym::NAVIGATION),
    ("near_me", sym::NEAR_ME),
    ("near_me_disabled", sym::NEAR_ME_DISABLED),
    ("nearby", sym::NEARBY),
    ("nearby_error", sym::NEARBY_ERROR),
    ("nearby_off", sym::NEARBY_OFF),
    ("nephrology", sym::NEPHROLOGY),
    ("nest_audio", sym::NEST_AUDIO),
    ("nest_cam_floodlight", sym::NEST_CAM_FLOODLIGHT),
    ("nest_cam_indoor", sym::NEST_CAM_INDOOR),
    ("nest_cam_iq", sym::NEST_CAM_IQ),
    ("nest_cam_iq_outdoor", sym::NEST_CAM_IQ_OUTDOOR),
    ("nest_cam_magnet_mount", sym::NEST_CAM_MAGNET_MOUNT),
    ("nest_cam_outdoor", sym::NEST_CAM_OUTDOOR),
    ("nest_cam_stand", sym::NEST_CAM_STAND),
    ("nest_cam_wall_mount", sym::NEST_CAM_WALL_MOUNT),
    ("nest_cam_wired_stand", sym::NEST_CAM_WIRED_STAND),
    ("nest_clock_farsight_analog", sym::NEST_CLOCK_FARSIGHT_ANALOG),
    ("nest_clock_farsight_digital", sym::NEST_CLOCK_FARSIGHT_DIGITAL),
    ("nest_connect", sym::NEST_CONNECT),
    ("nest_detect", sym::NEST_DETECT),
    ("nest_display", sym::NEST_DISPLAY),
    ("nest_display_max", sym::NEST_DISPLAY_MAX),
    ("nest_doorbell_visitor", sym::NEST_DOORBELL_VISITOR),
    ("nest_eco_leaf", sym::NEST_ECO_LEAF),
    ("nest_farsight_cool", sym::NEST_FARSIGHT_COOL),
    ("nest_farsight_dual", sym::NEST_FARSIGHT_DUAL),
    ("nest_farsight_eco", sym::NEST_FARSIGHT_ECO),
    ("nest_farsight_heat", sym::NEST_FARSIGHT_HEAT),
    ("nest_farsight_seasonal", sym::NEST_FARSIGHT_SEASONAL),
    ("nest_farsight_weather", sym::NEST_FARSIGHT_WEATHER),
    ("nest_found_savings", sym::NEST_FOUND_SAVINGS),
    ("nest_gale_wifi", sym::NEST_GALE_WIFI),
    ("nest_heat_link_e", sym::NEST_HEAT_LINK_E),
    ("nest_heat_link_gen_3", sym::NEST_HEAT_LINK_GEN_3),
    ("nest_hello_doorbell", sym::NEST_HELLO_DOORBELL),
    ("nest_locator_tag", sym::NEST_LOCATOR_TAG),
    ("nest_mini", sym::NEST_MINI),
    ("nest_multi_room", sym::NEST_MULTI_ROOM),
    ("nest_protect", sym::NEST_PROTECT),
    ("nest_remote", sym::NEST_REMOTE),
    ("nest_remote_comfort_sensor", sym::NEST_REMOTE_COMFORT_SENSOR),
    ("nest_secure_alarm", sym::NEST_SECURE_ALARM),
    ("nest_sunblock", sym::NEST_SUNBLOCK),
    ("nest_tag", sym::NEST_TAG),
    ("nest_thermostat", sym::NEST_THERMOSTAT),
    ("nest_thermostat_e_eu", sym::NEST_THERMOSTAT_E_EU),
    ("nest_thermostat_gen_3", sym::NEST_THERMOSTAT_GEN_3),
    ("nest_thermostat_sensor", sym::NEST_THERMOSTAT_SENSOR),
    ("nest_thermostat_sensor_eu", sym::NEST_THERMOSTAT_SENSOR_EU),
    ("nest_thermostat_zirconium_eu", sym::NEST_THERMOSTAT_ZIRCONIUM_EU),
    ("nest_true_radiant", sym::NEST_TRUE_RADIANT),
    ("nest_wake_on_approach", sym::NEST_WAKE_ON_APPROACH),
    ("nest_wake_on_press", sym::NEST_WAKE_ON_PRESS),
    ("nest_wifi_gale", sym::NEST_WIFI_GALE),
    ("nest_wifi_mistral", sym::NEST_WIFI_MISTRAL),
    ("nest_wifi_point", sym::NEST_WIFI_POINT),
    ("nest_wifi_point_vento", sym::NEST_WIFI_POINT_VENTO),
    ("nest_wifi_pro", sym::NEST_WIFI_PRO),
    ("nest_wifi_pro_2", sym::NEST_WIFI_PRO_2),
    ("nest_wifi_router", sym::NEST_WIFI_ROUTER),
    ("network_cell", sym::NETWORK_CELL),
    ("network_check", sym::NETWORK_CHECK),
    ("network_intel_node", sym::NETWORK_INTEL_NODE),
    ("network_intelligence", sym::NETWORK_INTELLIGENCE),
    ("network_intelligence_history", sym::NETWORK_INTELLIGENCE_HISTORY),
    ("network_intelligence_update", sym::NETWORK_INTELLIGENCE_UPDATE),
    ("network_locked", sym::NETWORK_LOCKED),
    ("network_manage", sym::NETWORK_MANAGE),
    ("network_node", sym::NETWORK_NODE),
    ("network_ping", sym::NETWORK_PING),
    ("network_wifi", sym::NETWORK_WIFI),
    ("network_wifi_1_bar", sym::NETWORK_WIFI_1_BAR),
    ("network_wifi_1_bar_locked", sym::NETWORK_WIFI_1_BAR_LOCKED),
    ("network_wifi_2_bar", sym::NETWORK_WIFI_2_BAR),
    ("network_wifi_2_bar_locked", sym::NETWORK_WIFI_2_BAR_LOCKED),
    ("network_wifi_3_bar", sym::NETWORK_WIFI_3_BAR),
    ("network_wifi_3_bar_locked", sym::NETWORK_WIFI_3_BAR_LOCKED),
    ("network_wifi_locked", sym::NETWORK_WIFI_LOCKED),
    ("neurology", sym::NEUROLOGY),
    ("new_label", sym::NEW_LABEL),
    ("new_releases", sym::NEW_RELEASES),
    ("new_window", sym::NEW_WINDOW),
    ("news", sym::NEWS),
    ("newsmode", sym::NEWSMODE),
    ("newspaper", sym::NEWSPAPER),
    ("newsstand", sym::NEWSSTAND),
    ("next_plan", sym::NEXT_PLAN),
    ("next_week", sym::NEXT_WEEK),
    ("nfc", sym::NFC),
    ("nfc_off", sym::NFC_OFF),
    ("night_shelter", sym::NIGHT_SHELTER),
    ("night_sight_auto", sym::NIGHT_SIGHT_AUTO),
    ("night_sight_auto_off", sym::NIGHT_SIGHT_AUTO_OFF),
    ("night_sight_max", sym::NIGHT_SIGHT_MAX),
    ("nightlife", sym::NIGHTLIFE),
    ("nightlight", sym::NIGHTLIGHT),
    ("nightlight_round", sym::NIGHTLIGHT_ROUND),
    ("nights_stay", sym::NIGHTS_STAY),
    ("no_accounts", sym::NO_ACCOUNTS),
    ("no_adult_content", sym::NO_ADULT_CONTENT),
    ("no_backpack", sym::NO_BACKPACK),
    ("no_crash", sym::NO_CRASH),
    ("no_drinks", sym::NO_DRINKS),
    ("no_encryption", sym::NO_ENCRYPTION),
    ("no_encryption_gmailerrorred", sym::NO_ENCRYPTION_GMAILERRORRED),
    ("no_flash", sym::NO_FLASH),
    ("no_food", sym::NO_FOOD),
    ("no_luggage", sym::NO_LUGGAGE),
    ("no_meals", sym::NO_MEALS),
    ("no_meeting_room", sym::NO_MEETING_ROOM),
    ("no_photography", sym::NO_PHOTOGRAPHY),
    ("no_sim", sym::NO_SIM),
    ("no_sound", sym::NO_SOUND),
    ("no_stroller", sym::NO_STROLLER),
    ("no_transfer", sym::NO_TRANSFER),
    ("noise_aware", sym::NOISE_AWARE),
    ("noise_control_off", sym::NOISE_CONTROL_OFF),
    ("noise_control_on", sym::NOISE_CONTROL_ON),
    ("nordic_walking", sym::NORDIC_WALKING),
    ("north", sym::NORTH),
    ("north_east", sym::NORTH_EAST),
    ("north_west", sym::NORTH_WEST),
    ("not_accessible", sym::NOT_ACCESSIBLE),
    ("not_accessible_forward", sym::NOT_ACCESSIBLE_FORWARD),
    ("not_interested", sym::NOT_INTERESTED),
    ("not_listed_location", sym::NOT_LISTED_LOCATION),
    ("not_started", sym::NOT_STARTED),
    ("note", sym::NOTE),
    ("note_add", sym::NOTE_ADD),
    ("note_alt", sym::NOTE_ALT),
    ("note_stack", sym::NOTE_STACK),
    ("note_stack_add", sym::NOTE_STACK_ADD),
    ("notes", sym::NOTES),
    ("notification_add", sym::NOTIFICATION_ADD),
    ("notification_audio", sym::NOTIFICATION_AUDIO),
    ("notification_audio_off", sym::NOTIFICATION_AUDIO_OFF),
    ("notification_important", sym::NOTIFICATION_IMPORTANT),
    ("notification_multiple", sym::NOTIFICATION_MULTIPLE),
    ("notification_settings", sym::NOTIFICATION_SETTINGS),
    ("notification_sound", sym::NOTIFICATION_SOUND),
    ("notifications", sym::NOTIFICATIONS),
    ("notifications_active", sym::NOTIFICATIONS_ACTIVE),
    ("notifications_none", sym::NOTIFICATIONS_NONE),
    ("notifications_off", sym::NOTIFICATIONS_OFF),
    ("notifications_paused", sym::NOTIFICATIONS_PAUSED),
    ("notifications_unread", sym::NOTIFICATIONS_UNREAD),
    ("numbers", sym::NUMBERS),
    ("nutrition", sym::NUTRITION),
    ("ods", sym::ODS),
    ("odt", sym::ODT),
    ("offline_bolt", sym::OFFLINE_BOLT),
    ("offline_pin", sym::OFFLINE_PIN),
    ("offline_pin_off", sym::OFFLINE_PIN_OFF),
    ("offline_share", sym::OFFLINE_SHARE),
    ("oil_barrel", sym::OIL_BARREL),
    ("okonomiyaki", sym::OKONOMIYAKI),
    ("on_device_training", sym::ON_DEVICE_TRAINING),
    ("on_hub_device", sym::ON_HUB_DEVICE),
    ("oncology", sym::ONCOLOGY),
    ("ondemand_video", sym::ONDEMAND_VIDEO),
    ("online_prediction", sym::ONLINE_PREDICTION),
    ("onsen", sym::ONSEN),
    ("opacity", sym::OPACITY),
    ("open_in_browser", sym::OPEN_IN_BROWSER),
    ("open_in_full", sym::OPEN_IN_FULL),
    ("open_in_new", sym::OPEN_IN_NEW),
    ("open_in_new_down", sym::OPEN_IN_NEW_DOWN),
    ("open_in_new_off", sym::OPEN_IN_NEW_OFF),
    ("open_in_phone", sym::OPEN_IN_PHONE),
    ("open_jam", sym::OPEN_JAM),
    ("open_run", sym::OPEN_RUN),
    ("open_with", sym::OPEN_WITH),
    ("ophthalmology", sym::OPHTHALMOLOGY),
    ("oral_disease", sym::ORAL_DISEASE),
    ("orbit", sym::ORBIT),
    ("order_approve", sym::ORDER_APPROVE),
    ("order_play", sym::ORDER_PLAY),
    ("orders", sym::ORDERS),
    ("orthopedics", sym::ORTHOPEDICS),
    ("other_admission", sym::OTHER_ADMISSION),
    ("other_houses", sym::OTHER_HOUSES),
    ("outbound", sym::OUTBOUND),
    ("outbox", sym::OUTBOX),
    ("outbox_alt", sym::OUTBOX_ALT),
    ("outdoor_garden", sym::OUTDOOR_GARDEN),
    ("outdoor_grill", sym::OUTDOOR_GRILL),
    ("outgoing_mail", sym::OUTGOING_MAIL),
    ("outlet", sym::OUTLET),
    ("outlined_flag", sym::OUTLINED_FLAG),
    ("outpatient", sym::OUTPATIENT),
    ("outpatient_med", sym::OUTPATIENT_MED),
    ("output", sym::OUTPUT),
    ("output_circle", sym::OUTPUT_CIRCLE),
    ("oven", sym::OVEN),
    ("oven_gen", sym::OVEN_GEN),
    ("overview", sym::OVERVIEW),
    ("overview_key", sym::OVERVIEW_KEY),
    ("owl", sym::OWL),
    ("oxygen_saturation", sym::OXYGEN_SATURATION),
    ("p2p", sym::P2P),
    ("pace", sym::PACE),
    ("pacemaker", sym::PACEMAKER),
    ("package", sym::PACKAGE),
    ("package_2", sym::PACKAGE_2),
    ("padding", sym::PADDING),
    ("padel", sym::PADEL),
    ("page_control", sym::PAGE_CONTROL),
    ("page_footer", sym::PAGE_FOOTER),
    ("page_header", sym::PAGE_HEADER),
    ("page_info", sym::PAGE_INFO),
    ("page_menu_ios", sym::PAGE_MENU_IOS),
    ("pageless", sym::PAGELESS),
    ("pages", sym::PAGES),
    ("pageview", sym::PAGEVIEW),
    ("paid", sym::PAID),
    ("palette", sym::PALETTE),
    ("pallet", sym::PALLET),
    ("pan_tool", sym::PAN_TOOL),
    ("pan_tool_alt", sym::PAN_TOOL_ALT),
    ("pan_zoom", sym::PAN_ZOOM),
    ("panorama", sym::PANORAMA),
    ("panorama_fish_eye", sym::PANORAMA_FISH_EYE),
    ("panorama_horizontal", sym::PANORAMA_HORIZONTAL),
    ("panorama_photosphere", sym::PANORAMA_PHOTOSPHERE),
    ("panorama_vertical", sym::PANORAMA_VERTICAL),
    ("panorama_wide_angle", sym::PANORAMA_WIDE_ANGLE),
    ("paragliding", sym::PARAGLIDING),
    ("parent_child_dining", sym::PARENT_CHILD_DINING),
    ("park", sym::PARK),
    ("parking_meter", sym::PARKING_METER),
    ("parking_sign", sym::PARKING_SIGN),
    ("parking_valet", sym::PARKING_VALET),
    ("partly_cloudy_day", sym::PARTLY_CLOUDY_DAY),
    ("partly_cloudy_night", sym::PARTLY_CLOUDY_NIGHT),
    ("partner_exchange", sym::PARTNER_EXCHANGE),
    ("partner_heart", sym::PARTNER_HEART),
    ("partner_reports", sym::PARTNER_REPORTS),
    ("party_mode", sym::PARTY_MODE),
    ("passkey", sym::PASSKEY),
    ("passport", sym::PASSPORT),
    ("password", sym::PASSWORD),
    ("password_2", sym::PASSWORD_2),
    ("password_2_off", sym::PASSWORD_2_OFF),
    ("patient_list", sym::PATIENT_LIST),
    ("pattern", sym::PATTERN),
    ("pause", sym::PAUSE),
    ("pause_circle", sym::PAUSE_CIRCLE),
    ("pause_circle_filled", sym::PAUSE_CIRCLE_FILLED),
    ("pause_circle_outline", sym::PAUSE_CIRCLE_OUTLINE),
    ("pause_presentation", sym::PAUSE_PRESENTATION),
    ("payment", sym::PAYMENT),
    ("payment_arrow_down", sym::PAYMENT_ARROW_DOWN),
    ("payment_card", sym::PAYMENT_CARD),
    ("payments", sym::PAYMENTS),
    ("pedal_bike", sym::PEDAL_BIKE),
    ("pediatrics", sym::PEDIATRICS),
    ("pen_size_1", sym::PEN_SIZE_1),
    ("pen_size_2", sym::PEN_SIZE_2),
    ("pen_size_3", sym::PEN_SIZE_3),
    ("pen_size_4", sym::PEN_SIZE_4),
    ("pen_size_5", sym::PEN_SIZE_5),
    ("pending", sym::PENDING),
    ("pending_actions", sym::PENDING_ACTIONS),
    ("pentagon", sym::PENTAGON),
    ("people", sym::PEOPLE),
    ("people_alt", sym::PEOPLE_ALT),
    ("people_outline", sym::PEOPLE_OUTLINE),
    ("people_size_decrease", sym::PEOPLE_SIZE_DECREASE),
    ("people_size_increase", sym::PEOPLE_SIZE_INCREASE),
    ("percent", sym::PERCENT),
    ("percent_discount", sym::PERCENT_DISCOUNT),
    ("performance_max", sym::PERFORMANCE_MAX),
    ("pergola", sym::PERGOLA),
    ("perm_camera_mic", sym::PERM_CAMERA_MIC),
    ("perm_contact_calendar", sym::PERM_CONTACT_CALENDAR),
    ("perm_data_setting", sym::PERM_DATA_SETTING),
    ("perm_device_information", sym::PERM_DEVICE_INFORMATION),
    ("perm_identity", sym::PERM_IDENTITY),
    ("perm_media", sym::PERM_MEDIA),
    ("perm_phone_msg", sym::PERM_PHONE_MSG),
    ("perm_scan_wifi", sym::PERM_SCAN_WIFI),
    ("person", sym::PERSON),
    ("person_2", sym::PERSON_2),
    ("person_3", sym::PERSON_3),
    ("person_4", sym::PERSON_4),
    ("person_add", sym::PERSON_ADD),
    ("person_add_alt", sym::PERSON_ADD_ALT),
    ("person_add_disabled", sym::PERSON_ADD_DISABLED),
    ("person_alert", sym::PERSON_ALERT),
    ("person_apron", sym::PERSON_APRON),
    ("person_book", sym::PERSON_BOOK),
    ("person_cancel", sym::PERSON_CANCEL),
    ("person_celebrate", sym::PERSON_CELEBRATE),
    ("person_check", sym::PERSON_CHECK),
    ("person_edit", sym::PERSON_EDIT),
    ("person_filled", sym::PERSON_FILLED),
    ("person_heart", sym::PERSON_HEART),
    ("person_off", sym::PERSON_OFF),
    ("person_outline", sym::PERSON_OUTLINE),
    ("person_pin", sym::PERSON_PIN),
    ("person_pin_circle", sym::PERSON_PIN_CIRCLE),
    ("person_play", sym::PERSON_PLAY),
    ("person_raised_hand", sym::PERSON_RAISED_HAND),
    ("person_remove", sym::PERSON_REMOVE),
    ("person_search", sym::PERSON_SEARCH),
    ("person_shield", sym::PERSON_SHIELD),
    ("person_text", sym::PERSON_TEXT),
    ("personal_bag", sym::PERSONAL_BAG),
    ("personal_bag_off", sym::PERSONAL_BAG_OFF),
    ("personal_bag_question", sym::PERSONAL_BAG_QUESTION),
    ("personal_injury", sym::PERSONAL_INJURY),
    ("personal_places", sym::PERSONAL_PLACES),
    ("personal_video", sym::PERSONAL_VIDEO),
    ("pest_control", sym::PEST_CONTROL),
    ("pest_control_rodent", sym::PEST_CONTROL_RODENT),
    ("pet_supplies", sym::PET_SUPPLIES),
    ("pets", sym::PETS),
    ("phishing", sym::PHISHING),
    ("phone", sym::PHONE),
    ("phone_alt", sym::PHONE_ALT),
    ("phone_android", sym::PHONE_ANDROID),
    ("phone_bluetooth_speaker", sym::PHONE_BLUETOOTH_SPEAKER),
    ("phone_callback", sym::PHONE_CALLBACK),
    ("phone_cancel", sym::PHONE_CANCEL),
    ("phone_disabled", sym::PHONE_DISABLED),
    ("phone_enabled", sym::PHONE_ENABLED),
    ("phone_forwarded", sym::PHONE_FORWARDED),
    ("phone_in_talk", sym::PHONE_IN_TALK),
    ("phone_iphone", sym::PHONE_IPHONE),
    ("phone_locked", sym::PHONE_LOCKED),
    ("phone_missed", sym::PHONE_MISSED),
    ("phone_paused", sym::PHONE_PAUSED),
    ("phonelink", sym::PHONELINK),
    ("phonelink_erase", sym::PHONELINK_ERASE),
    ("phonelink_lock", sym::PHONELINK_LOCK),
    ("phonelink_off", sym::PHONELINK_OFF),
    ("phonelink_ring", sym::PHONELINK_RING),
    ("phonelink_ring_off", sym::PHONELINK_RING_OFF),
    ("phonelink_setup", sym::PHONELINK_SETUP),
    ("photo", sym::PHOTO),
    ("photo_album", sym::PHOTO_ALBUM),
    ("photo_auto_merge", sym::PHOTO_AUTO_MERGE),
    ("photo_camera", sym::PHOTO_CAMERA),
    ("photo_camera_back", sym::PHOTO_CAMERA_BACK),
    ("photo_camera_front", sym::PHOTO_CAMERA_FRONT),
    ("photo_filter", sym::PHOTO_FILTER),
    ("photo_frame", sym::PHOTO_FRAME),
    ("photo_library", sym::PHOTO_LIBRARY),
    ("photo_prints", sym::PHOTO_PRINTS),
    ("photo_size_select_actual", sym::PHOTO_SIZE_SELECT_ACTUAL),
    ("photo_size_select_large", sym::PHOTO_SIZE_SELECT_LARGE),
    ("photo_size_select_small", sym::PHOTO_SIZE_SELECT_SMALL),
    ("php", sym::PHP),
    ("physical_therapy", sym::PHYSICAL_THERAPY),
    ("piano", sym::PIANO),
    ("piano_off", sym::PIANO_OFF),
    ("pickleball", sym::PICKLEBALL),
    ("picture_as_pdf", sym::PICTURE_AS_PDF),
    ("picture_in_picture", sym::PICTURE_IN_PICTURE),
    ("picture_in_picture_alt", sym::PICTURE_IN_PICTURE_ALT),
    ("picture_in_picture_center", sym::PICTURE_IN_PICTURE_CENTER),
    ("picture_in_picture_large", sym::PICTURE_IN_PICTURE_LARGE),
    ("picture_in_picture_medium", sym::PICTURE_IN_PICTURE_MEDIUM),
    ("picture_in_picture_mobile", sym::PICTURE_IN_PICTURE_MOBILE),
    ("picture_in_picture_off", sym::PICTURE_IN_PICTURE_OFF),
    ("picture_in_picture_small", sym::PICTURE_IN_PICTURE_SMALL),
    ("pie_chart", sym::PIE_CHART),
    ("pie_chart_filled", sym::PIE_CHART_FILLED),
    ("pie_chart_outline", sym::PIE_CHART_OUTLINE),
    ("pie_chart_outlined", sym::PIE_CHART_OUTLINED),
    ("pill", sym::PILL),
    ("pill_off", sym::PILL_OFF),
    ("pin", sym::PIN),
    ("pin_drop", sym::PIN_DROP),
    ("pin_end", sym::PIN_END),
    ("pin_history", sym::PIN_HISTORY),
    ("pin_invoke", sym::PIN_INVOKE),
    ("pin_road", sym::PIN_ROAD),
    ("pin_road_2", sym::PIN_ROAD_2),
    ("pinboard", sym::PINBOARD),
    ("pinboard_unread", sym::PINBOARD_UNREAD),
    ("pinch", sym::PINCH),
    ("pinch_zoom_in", sym::PINCH_ZOOM_IN),
    ("pinch_zoom_out", sym::PINCH_ZOOM_OUT),
    ("pip", sym::PIP),
    ("pip_exit", sym::PIP_EXIT),
    ("pivot_table_chart", sym::PIVOT_TABLE_CHART),
    ("place", sym::PLACE),
    ("place_item", sym::PLACE_ITEM),
    ("plagiarism", sym::PLAGIARISM),
    ("plane_contrails", sym::PLANE_CONTRAILS),
    ("planet", sym::PLANET),
    ("planner_banner_ad_pt", sym::PLANNER_BANNER_AD_PT),
    ("planner_review", sym::PLANNER_REVIEW),
    ("play_arrow", sym::PLAY_ARROW),
    ("play_circle", sym::PLAY_CIRCLE),
    ("play_disabled", sym::PLAY_DISABLED),
    ("play_for_work", sym::PLAY_FOR_WORK),
    ("play_lesson", sym::PLAY_LESSON),
    ("play_music", sym::PLAY_MUSIC),
    ("play_pause", sym::PLAY_PAUSE),
    ("play_shapes", sym::PLAY_SHAPES),
    ("playground", sym::PLAYGROUND),
    ("playground_2", sym::PLAYGROUND_2),
    ("playing_cards", sym::PLAYING_CARDS),
    ("playlist_add", sym::PLAYLIST_ADD),
    ("playlist_add_check", sym::PLAYLIST_ADD_CHECK),
    ("playlist_add_check_circle", sym::PLAYLIST_ADD_CHECK_CIRCLE),
    ("playlist_add_circle", sym::PLAYLIST_ADD_CIRCLE),
    ("playlist_play", sym::PLAYLIST_PLAY),
    ("playlist_remove", sym::PLAYLIST_REMOVE),
    ("plug_connect", sym::PLUG_CONNECT),
    ("plumbing", sym::PLUMBING),
    ("plus_one", sym::PLUS_ONE),
    ("podcasts", sym::PODCASTS),
    ("podiatry", sym::PODIATRY),
    ("podium", sym::PODIUM),
    ("point_of_sale", sym::POINT_OF_SALE),
    ("point_scan", sym::POINT_SCAN),
    ("poker_chip", sym::POKER_CHIP),
    ("policy", sym::POLICY),
    ("policy_alert", sym::POLICY_ALERT),
    ("poll", sym::POLL),
    ("polyline", sym::POLYLINE),
    ("polymer", sym::POLYMER),
    ("pool", sym::POOL),
    ("portable_wifi_off", sym::PORTABLE_WIFI_OFF),
    ("portrait", sym::PORTRAIT),
    ("position_bottom_left", sym::POSITION_BOTTOM_LEFT),
    ("position_bottom_right", sym::POSITION_BOTTOM_RIGHT),
    ("position_top_right", sym::POSITION_TOP_RIGHT),
    ("post", sym::POST),
    ("post_add", sym::POST_ADD),
    ("potted_plant", sym::POTTED_PLANT),
    ("power", sym::POWER),
    ("power_input", sym::POWER_INPUT),
    ("power_off", sym::POWER_OFF),
    ("power_rounded", sym::POWER_ROUNDED),
    ("power_settings_circle", sym::POWER_SETTINGS_CIRCLE),
    ("power_settings_new", sym::POWER_SETTINGS_NEW),
    ("prayer_times", sym::PRAYER_TIMES),
    ("precision_manufacturing", sym::PRECISION_MANUFACTURING),
    ("pregnancy", sym::PREGNANCY),
    ("pregnant_woman", sym::PREGNANT_WOMAN),
    ("preliminary", sym::PRELIMINARY),
    ("prescriptions", sym::PRESCRIPTIONS),
    ("present_to_all", sym::PRESENT_TO_ALL),
    ("preview", sym::PREVIEW),
    ("preview_off", sym::PREVIEW_OFF),
    ("price_change", sym::PRICE_CHANGE),
    ("price_check", sym::PRICE_CHECK),
    ("print", sym::PRINT),
    ("print_add", sym::PRINT_ADD),
    ("print_connect", sym::PRINT_CONNECT),
    ("print_disabled", sym::PRINT_DISABLED),
    ("print_error", sym::PRINT_ERROR),
    ("print_lock", sym::PRINT_LOCK),
    ("priority", sym::PRIORITY),
    ("priority_high", sym::PRIORITY_HIGH),
    ("privacy", sym::PRIVACY),
    ("privacy_tip", sym::PRIVACY_TIP),
    ("private_connectivity", sym::PRIVATE_CONNECTIVITY),
    ("problem", sym::PROBLEM),
    ("procedure", sym::PROCEDURE),
    ("process_chart", sym::PROCESS_CHART),
    ("production_quantity_limits", sym::PRODUCTION_QUANTITY_LIMITS),
    ("productivity", sym::PRODUCTIVITY),
    ("progress_activity", sym::PROGRESS_ACTIVITY),
    ("prompt_suggestion", sym::PROMPT_SUGGESTION),
    ("propane", sym::PROPANE),
    ("propane_tank", sym::PROPANE_TANK),
    ("psychiatry", sym::PSYCHIATRY),
    ("psychology", sym::PSYCHOLOGY),
    ("psychology_alt", sym::PSYCHOLOGY_ALT),
    ("public", sym::PUBLIC),
    ("public_off", sym::PUBLIC_OFF),
    ("publish", sym::PUBLISH),
    ("published_with_changes", sym::PUBLISHED_WITH_CHANGES),
    ("pulmonology", sym::PULMONOLOGY),
    ("pulse_alert", sym::PULSE_ALERT),
    ("punch_clock", sym::PUNCH_CLOCK),
    ("push_pin", sym::PUSH_PIN),
    ("qr_code", sym::QR_CODE),
    ("qr_code_2", sym::QR_CODE_2),
    ("qr_code_2_add", sym::QR_CODE_2_ADD),
    ("qr_code_scanner", sym::QR_CODE_SCANNER),
    ("query_builder", sym::QUERY_BUILDER),
    ("query_stats", sym::QUERY_STATS),
    ("question_answer", sym::QUESTION_ANSWER),
    ("question_exchange", sym::QUESTION_EXCHANGE),
    ("question_mark", sym::QUESTION_MARK),
    ("queue", sym::QUEUE),
    ("queue_music", sym::QUEUE_MUSIC),
    ("queue_play_next", sym::QUEUE_PLAY_NEXT),
    ("quick_phrases", sym::QUICK_PHRASES),
    ("quick_reference", sym::QUICK_REFERENCE),
    ("quick_reference_all", sym::QUICK_REFERENCE_ALL),
    ("quick_reorder", sym::QUICK_REORDER),
    ("quickreply", sym::QUICKREPLY),
    ("quiet_time", sym::QUIET_TIME),
    ("quiet_time_active", sym::QUIET_TIME_ACTIVE),
    ("quiz", sym::QUIZ),
    ("r_mobiledata", sym::R_MOBILEDATA),
    ("radar", sym::RADAR),
    ("radio", sym::RADIO),
    ("radio_button_checked", sym::RADIO_BUTTON_CHECKED),
    ("radio_button_partial", sym::RADIO_BUTTON_PARTIAL),
    ("radio_button_unchecked", sym::RADIO_BUTTON_UNCHECKED),
    ("radiology", sym::RADIOLOGY),
    ("railway_alert", sym::RAILWAY_ALERT),
    ("railway_alert_2", sym::RAILWAY_ALERT_2),
    ("rainy", sym::RAINY),
    ("rainy_heavy", sym::RAINY_HEAVY),
    ("rainy_light", sym::RAINY_LIGHT),
    ("rainy_snow", sym::RAINY_SNOW),
    ("ramen_dining", sym::RAMEN_DINING),
    ("ramp_left", sym::RAMP_LEFT),
    ("ramp_right", sym::RAMP_RIGHT),
    ("range_hood", sym::RANGE_HOOD),
    ("rate_review", sym::RATE_REVIEW),
    ("rate_review_rtl", sym::RATE_REVIEW_RTL),
    ("raven", sym::RAVEN),
    ("raw_off", sym::RAW_OFF),
    ("raw_on", sym::RAW_ON),
    ("read_more", sym::READ_MORE),
    ("readiness_score", sym::READINESS_SCORE),
    ("real_estate_agent", sym::REAL_ESTATE_AGENT),
    ("rear_camera", sym::REAR_CAMERA),
    ("rebase", sym::REBASE),
    ("rebase_edit", sym::REBASE_EDIT),
    ("receipt", sym::RECEIPT),
    ("receipt_long", sym::RECEIPT_LONG),
    ("receipt_long_off", sym::RECEIPT_LONG_OFF),
    ("recent_actors", sym::RECENT_ACTORS),
    ("recent_patient", sym::RECENT_PATIENT),
    ("recenter", sym::RECENTER),
    ("recommend", sym::RECOMMEND),
    ("record_voice_over", sym::RECORD_VOICE_OVER),
    ("rectangle", sym::RECTANGLE),
    ("rectangle_add", sym::RECTANGLE_ADD),
    ("recycling", sym::RECYCLING),
    ("redeem", sym::REDEEM),
    ("redo", sym::REDO),
    ("reduce_capacity", sym::REDUCE_CAPACITY),
    ("refresh", sym::REFRESH),
    ("regular_expression", sym::REGULAR_EXPRESSION),
    ("relax", sym::RELAX),
    ("release_alert", sym::RELEASE_ALERT),
    ("remember_me", sym::REMEMBER_ME),
    ("reminder", sym::REMINDER),
    ("reminders_alt", sym::REMINDERS_ALT),
    ("remote_gen", sym::REMOTE_GEN),
    ("remove", sym::REMOVE),
    ("remove_circle", sym::REMOVE_CIRCLE),
    ("remove_circle_outline", sym::REMOVE_CIRCLE_OUTLINE),
    ("remove_done", sym::REMOVE_DONE),
    ("remove_from_queue", sym::REMOVE_FROM_QUEUE),
    ("remove_moderator", sym::REMOVE_MODERATOR),
    ("remove_red_eye", sym::REMOVE_RED_EYE),
    ("remove_road", sym::REMOVE_ROAD),
    ("remove_selection", sym::REMOVE_SELECTION),
    ("remove_shopping_cart", sym::REMOVE_SHOPPING_CART),
    ("reopen_window", sym::REOPEN_WINDOW),
    ("reorder", sym::REORDER),
    ("repartition", sym::REPARTITION),
    ("repeat", sym::REPEAT),
    ("repeat_on", sym::REPEAT_ON),
    ("repeat_one", sym::REPEAT_ONE),
    ("repeat_one_on", sym::REPEAT_ONE_ON),
    ("replace_audio", sym::REPLACE_AUDIO),
    ("replace_image", sym::REPLACE_IMAGE),
    ("replace_video", sym::REPLACE_VIDEO),
    ("replay", sym::REPLAY),
    ("replay_10", sym::REPLAY_10),
    ("replay_30", sym::REPLAY_30),
    ("replay_5", sym::REPLAY_5),
    ("replay_circle_filled", sym::REPLAY_CIRCLE_FILLED),
    ("reply", sym::REPLY),
    ("reply_all", sym::REPLY_ALL),
    ("report", sym::REPORT),
    ("report_gmailerrorred", sym::REPORT_GMAILERRORRED),
    ("report_off", sym::REPORT_OFF),
    ("report_problem", sym::REPORT_PROBLEM),
    ("request_page", sym::REQUEST_PAGE),
    ("request_quote", sym::REQUEST_QUOTE),
    ("reset_brightness", sym::RESET_BRIGHTNESS),
    ("reset_colors", sym::RESET_COLORS),
    ("reset_exposure", sym::RESET_EXPOSURE),
    ("reset_focus", sym::RESET_FOCUS),
    ("reset_image", sym::RESET_IMAGE),
    ("reset_iso", sym::RESET_ISO),
    ("reset_settings", sym::RESET_SETTINGS),
    ("reset_shadow", sym::RESET_SHADOW),
    ("reset_shutter_speed", sym::RESET_SHUTTER_SPEED),
    ("reset_tv", sym::RESET_TV),
    ("reset_white_balance", sym::RESET_WHITE_BALANCE),
    ("reset_wrench", sym::RESET_WRENCH),
    ("resize", sym::RESIZE),
    ("resize_window", sym::RESIZE_WINDOW),
    ("respiratory_rate", sym::RESPIRATORY_RATE),
    ("responsive_layout", sym::RESPONSIVE_LAYOUT),
    ("rest_area", sym::REST_AREA),
    ("restart_alt", sym::RESTART_ALT),
    ("restaurant", sym::RESTAURANT),
    ("restaurant_menu", sym::RESTAURANT_MENU),
    ("restore", sym::RESTORE),
    ("restore_from_trash", sym::RESTORE_FROM_TRASH),
    ("restore_page", sym::RESTORE_PAGE),
    ("resume", sym::RESUME),
    ("reviews", sym::REVIEWS),
    ("rewarded_ads", sym::REWARDED_ADS),
    ("rheumatology", sym::RHEUMATOLOGY),
    ("rib_cage", sym::RIB_CAGE),
    ("rice_bowl", sym::RICE_BOWL),
    ("right_click", sym::RIGHT_CLICK),
    ("right_panel_close", sym::RIGHT_PANEL_CLOSE),
    ("right_panel_open", sym::RIGHT_PANEL_OPEN),
    ("ring_volume", sym::RING_VOLUME),
    ("ring_volume_filled", sym::RING_VOLUME_FILLED),
    ("ripples", sym::RIPPLES),
    ("road", sym::ROAD),
    ("robot", sym::ROBOT),
    ("robot_2", sym::ROBOT_2),
    ("rocket", sym::ROCKET),
    ("rocket_launch", sym::ROCKET_LAUNCH),
    ("roller_shades", sym::ROLLER_SHADES),
    ("roller_shades_closed", sym::ROLLER_SHADES_CLOSED),
    ("roller_skating", sym::ROLLER_SKATING),
    ("roofing", sym::ROOFING),
    ("room", sym::ROOM),
    ("room_preferences", sym::ROOM_PREFERENCES),
    ("room_service", sym::ROOM_SERVICE),
    ("rotate_90_degrees_ccw", sym::ROTATE_90_DEGREES_CCW),
    ("rotate_90_degrees_cw", sym::ROTATE_90_DEGREES_CW),
    ("rotate_auto", sym::ROTATE_AUTO),
    ("rotate_left", sym::ROTATE_LEFT),
    ("rotate_right", sym::ROTATE_RIGHT),
    ("roundabout_left", sym::ROUNDABOUT_LEFT),
    ("roundabout_right", sym::ROUNDABOUT_RIGHT),
    ("rounded_corner", sym::ROUNDED_CORNER),
    ("route", sym::ROUTE),
    ("router", sym::ROUTER),
    ("router_off", sym::ROUTER_OFF),
    ("routine", sym::ROUTINE),
    ("rowing", sym::ROWING),
    ("rss_feed", sym::RSS_FEED),
    ("rsvp", sym::RSVP),
    ("rtt", sym::RTT),
    ("rubric", sym::RUBRIC),
    ("rule", sym::RULE),
    ("rule_folder", sym::RULE_FOLDER),
    ("rule_settings", sym::RULE_SETTINGS),
    ("run_circle", sym::RUN_CIRCLE),
    ("running_with_errors", sym::RUNNING_WITH_ERRORS),
    ("rv_hookup", sym::RV_HOOKUP),
    ("safety_check", sym::SAFETY_CHECK),
    ("safety_check_off", sym::SAFETY_CHECK_OFF),
    ("safety_divider", sym::SAFETY_DIVIDER),
    ("sailing", sym::SAILING),
    ("salinity", sym::SALINITY),
    ("sanitizer", sym::SANITIZER),
    ("satellite", sym::SATELLITE),
    ("satellite_alt", sym::SATELLITE_ALT),
    ("sauna", sym::SAUNA),
    ("save", sym::SAVE),
    ("save_alt", sym::SAVE_ALT),
    ("save_as", sym::SAVE_AS),
    ("save_clock", sym::SAVE_CLOCK),
    ("saved_search", sym::SAVED_SEARCH),
    ("savings", sym::SAVINGS),
    ("scale", sym::SCALE),
    ("scan", sym::SCAN),
    ("scan_delete", sym::SCAN_DELETE),
    ("scanner", sym::SCANNER),
    ("scatter_plot", sym::SCATTER_PLOT),
    ("scene", sym::SCENE),
    ("schedule", sym::SCHEDULE),
    ("schedule_send", sym::SCHEDULE_SEND),
    ("schema", sym::SCHEMA),
    ("school", sym::SCHOOL),
    ("science", sym::SCIENCE),
    ("science_off", sym::SCIENCE_OFF),
    ("scooter", sym::SCOOTER),
    ("score", sym::SCORE),
    ("scoreboard", sym::SCOREBOARD),
    ("screen_lock_landscape", sym::SCREEN_LOCK_LANDSCAPE),
    ("screen_lock_portrait", sym::SCREEN_LOCK_PORTRAIT),
    ("screen_lock_rotation", sym::SCREEN_LOCK_ROTATION),
    ("screen_record", sym::SCREEN_RECORD),
    ("screen_rotation", sym::SCREEN_ROTATION),
    ("screen_rotation_alt", sym::SCREEN_ROTATION_ALT),
    ("screen_rotation_up", sym::SCREEN_ROTATION_UP),
    ("screen_search_desktop", sym::SCREEN_SEARCH_DESKTOP),
    ("screen_share", sym::SCREEN_SHARE),
    ("screenshot", sym::SCREENSHOT),
    ("screenshot_frame", sym::SCREENSHOT_FRAME),
    ("screenshot_frame_2", sym::SCREENSHOT_FRAME_2),
    ("screenshot_keyboard", sym::SCREENSHOT_KEYBOARD),
    ("screenshot_monitor", sym::SCREENSHOT_MONITOR),
    ("screenshot_region", sym::SCREENSHOT_REGION),
    ("screenshot_tablet", sym::SCREENSHOT_TABLET),
    ("script", sym::SCRIPT),
    ("scrollable_header", sym::SCROLLABLE_HEADER),
    ("scuba_diving", sym::SCUBA_DIVING),
    ("sd", sym::SD),
    ("sd_card", sym::SD_CARD),
    ("sd_card_alert", sym::SD_CARD_ALERT),
    ("sd_storage", sym::SD_STORAGE),
    ("sdk", sym::SDK),
    ("search", sym::SEARCH),
    ("search_activity", sym::SEARCH_ACTIVITY),
    ("search_check", sym::SEARCH_CHECK),
    ("search_check_2", sym::SEARCH_CHECK_2),
    ("search_gear", sym::SEARCH_GEAR),
    ("search_hands_free", sym::SEARCH_HANDS_FREE),
    ("search_insights", sym::SEARCH_INSIGHTS),
    ("search_off", sym::SEARCH_OFF),
    ("seat_cool_left", sym::SEAT_COOL_LEFT),
    ("seat_cool_right", sym::SEAT_COOL_RIGHT),
    ("seat_heat_left", sym::SEAT_HEAT_LEFT),
    ("seat_heat_right", sym::SEAT_HEAT_RIGHT),
    ("seat_read", sym::SEAT_READ),
    ("seat_vent_left", sym::SEAT_VENT_LEFT),
    ("seat_vent_right", sym::SEAT_VENT_RIGHT),
    ("seat_window", sym::SEAT_WINDOW),
    ("security", sym::SECURITY),
    ("security_key", sym::SECURITY_KEY),
    ("security_update", sym::SECURITY_UPDATE),
    ("security_update_good", sym::SECURITY_UPDATE_GOOD),
    ("security_update_warning", sym::SECURITY_UPDATE_WARNING),
    ("segment", sym::SEGMENT),
    ("select", sym::SELECT),
    ("select_all", sym::SELECT_ALL),
    ("select_check_box", sym::SELECT_CHECK_BOX),
    ("select_to_speak", sym::SELECT_TO_SPEAK),
    ("select_window", sym::SELECT_WINDOW),
    ("select_window_2", sym::SELECT_WINDOW_2),
    ("select_window_off", sym::SELECT_WINDOW_OFF),
    ("self_care", sym::SELF_CARE),
    ("self_improvement", sym::SELF_IMPROVEMENT),
    ("sell", sym::SELL),
    ("sell_cloud", sym::SELL_CLOUD),
    ("send", sym::SEND),
    ("send_and_archive", sym::SEND_AND_ARCHIVE),
    ("send_money", sym::SEND_MONEY),
    ("send_time_extension", sym::SEND_TIME_EXTENSION),
    ("send_to_mobile", sym::SEND_TO_MOBILE),
    ("sensor_door", sym::SENSOR_DOOR),
    ("sensor_occupied", sym::SENSOR_OCCUPIED),
    ("sensor_window", sym::SENSOR_WINDOW),
    ("sensors", sym::SENSORS),
    ("sensors_krx", sym::SENSORS_KRX),
    ("sensors_krx_off", sym::SENSORS_KRX_OFF),
    ("sensors_off", sym::SENSORS_OFF),
    ("sentiment_calm", sym::SENTIMENT_CALM),
    ("sentiment_content", sym::SENTIMENT_CONTENT),
    ("sentiment_dissatisfied", sym::SENTIMENT_DISSATISFIED),
    ("sentiment_excited", sym::SENTIMENT_EXCITED),
    ("sentiment_extremely_dissatisfied", sym::SENTIMENT_EXTREMELY_DISSATISFIED),
    ("sentiment_frustrated", sym::SENTIMENT_FRUSTRATED),
    ("sentiment_neutral", sym::SENTIMENT_NEUTRAL),
    ("sentiment_sad", sym::SENTIMENT_SAD),
    ("sentiment_satisfied", sym::SENTIMENT_SATISFIED),
    ("sentiment_satisfied_alt", sym::SENTIMENT_SATISFIED_ALT),
    ("sentiment_stressed", sym::SENTIMENT_STRESSED),
    ("sentiment_very_dissatisfied", sym::SENTIMENT_VERY_DISSATISFIED),
    ("sentiment_very_satisfied", sym::SENTIMENT_VERY_SATISFIED),
    ("sentiment_worried", sym::SENTIMENT_WORRIED),
    ("serif", sym::SERIF),
    ("server_person", sym::SERVER_PERSON),
    ("service_toolbox", sym::SERVICE_TOOLBOX),
    ("set_meal", sym::SET_MEAL),
    ("settings", sym::SETTINGS),
    ("settings_accessibility", sym::SETTINGS_ACCESSIBILITY),
    ("settings_account_box", sym::SETTINGS_ACCOUNT_BOX),
    ("settings_alert", sym::SETTINGS_ALERT),
    ("settings_applications", sym::SETTINGS_APPLICATIONS),
    ("settings_b_roll", sym::SETTINGS_B_ROLL),
    ("settings_backup_restore", sym::SETTINGS_BACKUP_RESTORE),
    ("settings_bluetooth", sym::SETTINGS_BLUETOOTH),
    ("settings_brightness", sym::SETTINGS_BRIGHTNESS),
    ("settings_cell", sym::SETTINGS_CELL),
    ("settings_cinematic_blur", sym::SETTINGS_CINEMATIC_BLUR),
    ("settings_ethernet", sym::SETTINGS_ETHERNET),
    ("settings_heart", sym::SETTINGS_HEART),
    ("settings_input_antenna", sym::SETTINGS_INPUT_ANTENNA),
    ("settings_input_component", sym::SETTINGS_INPUT_COMPONENT),
    ("settings_input_composite", sym::SETTINGS_INPUT_COMPOSITE),
    ("settings_input_hdmi", sym::SETTINGS_INPUT_HDMI),
    ("settings_input_svideo", sym::SETTINGS_INPUT_SVIDEO),
    ("settings_motion_mode", sym::SETTINGS_MOTION_MODE),
    ("settings_night_sight", sym::SETTINGS_NIGHT_SIGHT),
    ("settings_overscan", sym::SETTINGS_OVERSCAN),
    ("settings_panorama", sym::SETTINGS_PANORAMA),
    ("settings_phone", sym::SETTINGS_PHONE),
    ("settings_photo_camera", sym::SETTINGS_PHOTO_CAMERA),
    ("settings_power", sym::SETTINGS_POWER),
    ("settings_remote", sym::SETTINGS_REMOTE),
    ("settings_screen", sym::SETTINGS_SCREEN),
    ("settings_seating", sym::SETTINGS_SEATING),
    ("settings_slow_motion", sym::SETTINGS_SLOW_MOTION),
    ("settings_suggest", sym::SETTINGS_SUGGEST),
    ("settings_system_daydream", sym::SETTINGS_SYSTEM_DAYDREAM),
    ("settings_timelapse", sym::SETTINGS_TIMELAPSE),
    ("settings_video_camera", sym::SETTINGS_VIDEO_CAMERA),
    ("settings_voice", sym::SETTINGS_VOICE),
    ("settop_component", sym::SETTOP_COMPONENT),
    ("severe_cold", sym::SEVERE_COLD),
    ("shades", sym::SHADES),
    ("shades_closed", sym::SHADES_CLOSED),
    ("shadow", sym::SHADOW),
    ("shadow_add", sym::SHADOW_ADD),
    ("shadow_minus", sym::SHADOW_MINUS),
    ("shape_line", sym::SHAPE_LINE),
    ("shape_recognition", sym::SHAPE_RECOGNITION),
    ("shapes", sym::SHAPES),
    ("share", sym::SHARE),
    ("share_eta", sym::SHARE_ETA),
    ("share_location", sym::SHARE_LOCATION),
    ("share_off", sym::SHARE_OFF),
    ("share_reviews", sym::SHARE_REVIEWS),
    ("share_windows", sym::SHARE_WINDOWS),
    ("shaved_ice", sym::SHAVED_ICE),
    ("sheets_rtl", sym::SHEETS_RTL),
    ("shelf_auto_hide", sym::SHELF_AUTO_HIDE),
    ("shelf_position", sym::SHELF_POSITION),
    ("shelves", sym::SHELVES),
    ("shield", sym::SHIELD),
    ("shield_card", sym::SHIELD_CARD),
    ("shield_lock", sym::SHIELD_LOCK),
    ("shield_locked", sym::SHIELD_LOCKED),
    ("shield_moon", sym::SHIELD_MOON),
    ("shield_person", sym::SHIELD_PERSON),
    ("shield_question", sym::SHIELD_QUESTION),
    ("shield_radar", sym::SHIELD_RADAR),
    ("shield_toggle", sym::SHIELD_TOGGLE),
    ("shield_watch", sym::SHIELD_WATCH),
    ("shield_with_heart", sym::SHIELD_WITH_HEART),
    ("shield_with_house", sym::SHIELD_WITH_HOUSE),
    ("shift", sym::SHIFT),
    ("shift_lock", sym::SHIFT_LOCK),
    ("shift_lock_off", sym::SHIFT_LOCK_OFF),
    ("shoe_cleats", sym::SHOE_CLEATS),
    ("shop", sym::SHOP),
    ("shop_2", sym::SHOP_2),
    ("shop_two", sym::SHOP_TWO),
    ("shopping_bag", sym::SHOPPING_BAG),
    ("shopping_bag_speed", sym::SHOPPING_BAG_SPEED),
    ("shopping_basket", sym::SHOPPING_BASKET),
    ("shopping_cart", sym::SHOPPING_CART),
    ("shopping_cart_checkout", sym::SHOPPING_CART_CHECKOUT),
    ("shopping_cart_off", sym::SHOPPING_CART_OFF),
    ("shoppingmode", sym::SHOPPINGMODE),
    ("short_stay", sym::SHORT_STAY),
    ("short_text", sym::SHORT_TEXT),
    ("shortcut", sym::SHORTCUT),
    ("show_chart", sym::SHOW_CHART),
    ("shower", sym::SHOWER),
    ("shuffle", sym::SHUFFLE),
    ("shuffle_on", sym::SHUFFLE_ON),
    ("shutter_speed", sym::SHUTTER_SPEED),
    ("shutter_speed_add", sym::SHUTTER_SPEED_ADD),
    ("shutter_speed_minus", sym::SHUTTER_SPEED_MINUS),
    ("sick", sym::SICK),
    ("side_navigation", sym::SIDE_NAVIGATION),
    ("sign_language", sym::SIGN_LANGUAGE),
    ("sign_language_2", sym::SIGN_LANGUAGE_2),
    ("sign_language_off", sym::SIGN_LANGUAGE_OFF),
    ("signal_cellular_0_bar", sym::SIGNAL_CELLULAR_0_BAR),
    ("signal_cellular_1_bar", sym::SIGNAL_CELLULAR_1_BAR),
    ("signal_cellular_2_bar", sym::SIGNAL_CELLULAR_2_BAR),
    ("signal_cellular_3_bar", sym::SIGNAL_CELLULAR_3_BAR),
    ("signal_cellular_4_bar", sym::SIGNAL_CELLULAR_4_BAR),
    ("signal_cellular_add", sym::SIGNAL_CELLULAR_ADD),
    ("signal_cellular_alt", sym::SIGNAL_CELLULAR_ALT),
    ("signal_cellular_alt_1_bar", sym::SIGNAL_CELLULAR_ALT_1_BAR),
    ("signal_cellular_alt_2_bar", sym::SIGNAL_CELLULAR_ALT_2_BAR),
    ("signal_cellular_alt_off", sym::SIGNAL_CELLULAR_ALT_OFF),
    ("signal_cellular_connected_no_internet_0_bar", sym::SIGNAL_CELLULAR_CONNECTED_NO_INTERNET_0_BAR),
    ("signal_cellular_connected_no_internet_4_bar", sym::SIGNAL_CELLULAR_CONNECTED_NO_INTERNET_4_BAR),
    ("signal_cellular_no_sim", sym::SIGNAL_CELLULAR_NO_SIM),
    ("signal_cellular_nodata", sym::SIGNAL_CELLULAR_NODATA),
    ("signal_cellular_null", sym::SIGNAL_CELLULAR_NULL),
    ("signal_cellular_off", sym::SIGNAL_CELLULAR_OFF),
    ("signal_cellular_pause", sym::SIGNAL_CELLULAR_PAUSE),
    ("signal_disconnected", sym::SIGNAL_DISCONNECTED),
    ("signal_wifi_0_bar", sym::SIGNAL_WIFI_0_BAR),
    ("signal_wifi_4_bar", sym::SIGNAL_WIFI_4_BAR),
    ("signal_wifi_4_bar_lock", sym::SIGNAL_WIFI_4_BAR_LOCK),
    ("signal_wifi_bad", sym::SIGNAL_WIFI_BAD),
    ("signal_wifi_connected_no_internet_4", sym::SIGNAL_WIFI_CONNECTED_NO_INTERNET_4),
    ("signal_wifi_off", sym::SIGNAL_WIFI_OFF),
    ("signal_wifi_statusbar_4_bar", sym::SIGNAL_WIFI_STATUSBAR_4_BAR),
    ("signal_wifi_statusbar_not_connected", sym::SIGNAL_WIFI_STATUSBAR_NOT_CONNECTED),
    ("signal_wifi_statusbar_null", sym::SIGNAL_WIFI_STATUSBAR_NULL),
    ("signature", sym::SIGNATURE),
    ("signpost", sym::SIGNPOST),
    ("sim_card", sym::SIM_CARD),
    ("sim_card_alert", sym::SIM_CARD_ALERT),
    ("sim_card_download", sym::SIM_CARD_DOWNLOAD),
    ("sim_card_lock", sym::SIM_CARD_LOCK),
    ("simulation", sym::SIMULATION),
    ("single_arrow", sym::SINGLE_ARROW),
    ("single_bed", sym::SINGLE_BED),
    ("sip", sym::SIP),
    ("siren", sym::SIREN),
    ("siren_check", sym::SIREN_CHECK),
    ("siren_open", sym::SIREN_OPEN),
    ("siren_question", sym::SIREN_QUESTION),
    ("skateboarding", sym::SKATEBOARDING),
    ("skeleton", sym::SKELETON),
    ("skillet", sym::SKILLET),
    ("skillet_cooktop", sym::SKILLET_COOKTOP),
    ("skip_next", sym::SKIP_NEXT),
    ("skip_previous", sym::SKIP_PREVIOUS),
    ("skull", sym::SKULL),
    ("skull_list", sym::SKULL_LIST),
    ("slab_serif", sym::SLAB_SERIF),
    ("sledding", sym::SLEDDING),
    ("sleep", sym::SLEEP),
    ("sleep_score", sym::SLEEP_SCORE),
    ("slide_library", sym::SLIDE_LIBRARY),
    ("sliders", sym::SLIDERS),
    ("slideshow", sym::SLIDESHOW),
    ("slow_motion_video", sym::SLOW_MOTION_VIDEO),
    ("smart_button", sym::SMART_BUTTON),
    ("smart_card_reader", sym::SMART_CARD_READER),
    ("smart_card_reader_off", sym::SMART_CARD_READER_OFF),
    ("smart_display", sym::SMART_DISPLAY),
    ("smart_outlet", sym::SMART_OUTLET),
    ("smart_screen", sym::SMART_SCREEN),
    ("smart_toy", sym::SMART_TOY),
    ("smartphone", sym::SMARTPHONE),
    ("smartphone_camera", sym::SMARTPHONE_CAMERA),
    ("smb_share", sym::SMB_SHARE),
    ("smoke_free", sym::SMOKE_FREE),
    ("smoking_rooms", sym::SMOKING_ROOMS),
    ("sms", sym::SMS),
    ("sms_failed", sym::SMS_FAILED),
    ("snail", sym::SNAIL),
    ("snippet_folder", sym::SNIPPET_FOLDER),
    ("snooze", sym::SNOOZE),
    ("snowboarding", sym::SNOWBOARDING),
    ("snowflake", sym::SNOWFLAKE),
    ("snowing", sym::SNOWING),
    ("snowing_heavy", sym::SNOWING_HEAVY),
    ("snowmobile", sym::SNOWMOBILE),
    ("snowshoeing", sym::SNOWSHOEING),
    ("soap", sym::SOAP),
    ("soba", sym::SOBA),
    ("social_distance", sym::SOCIAL_DISTANCE),
    ("social_leaderboard", sym::SOCIAL_LEADERBOARD),
    ("solar_power", sym::SOLAR_POWER),
    ("solo_dining", sym::SOLO_DINING),
    ("sort", sym::SORT),
    ("sort_by_alpha", sym::SORT_BY_ALPHA),
    ("sos", sym::SOS),
    ("sound_detection_dog_barking", sym::SOUND_DETECTION_DOG_BARKING),
    ("sound_detection_glass_break", sym::SOUND_DETECTION_GLASS_BREAK),
    ("sound_detection_loud_sound", sym::SOUND_DETECTION_LOUD_SOUND),
    ("sound_sampler", sym::SOUND_SAMPLER),
    ("soundbar", sym::SOUNDBAR),
    ("soup_kitchen", sym::SOUP_KITCHEN),
    ("source", sym::SOURCE),
    ("source_environment", sym::SOURCE_ENVIRONMENT),
    ("source_notes", sym::SOURCE_NOTES),
    ("south", sym::SOUTH),
    ("south_america", sym::SOUTH_AMERICA),
    ("south_east", sym::SOUTH_EAST),
    ("south_west", sym::SOUTH_WEST),
    ("spa", sym::SPA),
    ("space_bar", sym::SPACE_BAR),
    ("space_dashboard", sym::SPACE_DASHBOARD),
    ("space_dashboard_2", sym::SPACE_DASHBOARD_2),
    ("spatial_audio", sym::SPATIAL_AUDIO),
    ("spatial_audio_off", sym::SPATIAL_AUDIO_OFF),
    ("spatial_gallery", sym::SPATIAL_GALLERY),
    ("spatial_speaker", sym::SPATIAL_SPEAKER),
    ("spatial_tracking", sym::SPATIAL_TRACKING),
    ("speaker", sym::SPEAKER),
    ("speaker_2", sym::SPEAKER_2),
    ("speaker_3", sym::SPEAKER_3),
    ("speaker_group", sym::SPEAKER_GROUP),
    ("speaker_notes", sym::SPEAKER_NOTES),
    ("speaker_notes_off", sym::SPEAKER_NOTES_OFF),
    ("speaker_phone", sym::SPEAKER_PHONE),
    ("special_character", sym::SPECIAL_CHARACTER),
    ("specific_gravity", sym::SPECIFIC_GRAVITY),
    ("speech_to_text", sym::SPEECH_TO_TEXT),
    ("speech_to_text_2", sym::SPEECH_TO_TEXT_2),
    ("speed", sym::SPEED),
    ("speed_0_25", sym::SPEED_0_25),
    ("speed_0_2x", sym::SPEED_0_2X),
    ("speed_0_5", sym::SPEED_0_5),
    ("speed_0_5x", sym::SPEED_0_5X),
    ("speed_0_75", sym::SPEED_0_75),
    ("speed_0_7x", sym::SPEED_0_7X),
    ("speed_1_2", sym::SPEED_1_2),
    ("speed_1_25", sym::SPEED_1_25),
    ("speed_1_2x", sym::SPEED_1_2X),
    ("speed_1_5", sym::SPEED_1_5),
    ("speed_1_5x", sym::SPEED_1_5X),
    ("speed_1_75", sym::SPEED_1_75),
    ("speed_1_7x", sym::SPEED_1_7X),
    ("speed_2", sym::SPEED_2),
    ("speed_2x", sym::SPEED_2X),
    ("speed_3", sym::SPEED_3),
    ("speed_4", sym::SPEED_4),
    ("speed_camera", sym::SPEED_CAMERA),
    ("spellcheck", sym::SPELLCHECK),
    ("split_scene", sym::SPLIT_SCENE),
    ("split_scene_2", sym::SPLIT_SCENE_2),
    ("split_scene_down", sym::SPLIT_SCENE_DOWN),
    ("split_scene_left", sym::SPLIT_SCENE_LEFT),
    ("split_scene_right", sym::SPLIT_SCENE_RIGHT),
    ("split_scene_up", sym::SPLIT_SCENE_UP),
    ("splitscreen", sym::SPLITSCREEN),
    ("splitscreen_add", sym::SPLITSCREEN_ADD),
    ("splitscreen_bottom", sym::SPLITSCREEN_BOTTOM),
    ("splitscreen_landscape", sym::SPLITSCREEN_LANDSCAPE),
    ("splitscreen_landscape_add", sym::SPLITSCREEN_LANDSCAPE_ADD),
    ("splitscreen_left", sym::SPLITSCREEN_LEFT),
    ("splitscreen_portrait", sym::SPLITSCREEN_PORTRAIT),
    ("splitscreen_right", sym::SPLITSCREEN_RIGHT),
    ("splitscreen_top", sym::SPLITSCREEN_TOP),
    ("splitscreen_vertical_add", sym::SPLITSCREEN_VERTICAL_ADD),
    ("spo2", sym::SPO2),
    ("spoke", sym::SPOKE),
    ("sports", sym::SPORTS),
    ("sports_and_outdoors", sym::SPORTS_AND_OUTDOORS),
    ("sports_bar", sym::SPORTS_BAR),
    ("sports_baseball", sym::SPORTS_BASEBALL),
    ("sports_basketball", sym::SPORTS_BASKETBALL),
    ("sports_cricket", sym::SPORTS_CRICKET),
    ("sports_esports", sym::SPORTS_ESPORTS),
    ("sports_football", sym::SPORTS_FOOTBALL),
    ("sports_golf", sym::SPORTS_GOLF),
    ("sports_gymnastics", sym::SPORTS_GYMNASTICS),
    ("sports_handball", sym::SPORTS_HANDBALL),
    ("sports_hockey", sym::SPORTS_HOCKEY),
    ("sports_kabaddi", sym::SPORTS_KABADDI),
    ("sports_martial_arts", sym::SPORTS_MARTIAL_ARTS),
    ("sports_mma", sym::SPORTS_MMA),
    ("sports_motorsports", sym::SPORTS_MOTORSPORTS),
    ("sports_rugby", sym::SPORTS_RUGBY),
    ("sports_score", sym::SPORTS_SCORE),
    ("sports_soccer", sym::SPORTS_SOCCER),
    ("sports_tennis", sym::SPORTS_TENNIS),
    ("sports_volleyball", sym::SPORTS_VOLLEYBALL),
    ("sprinkler", sym::SPRINKLER),
    ("sprint", sym::SPRINT),
    ("sql", sym::SQL),
    ("square", sym::SQUARE),
    ("square_circle", sym::SQUARE_CIRCLE),
    ("square_dot", sym::SQUARE_DOT),
    ("square_foot", sym::SQUARE_FOOT),
    ("ssid_chart", sym::SSID_CHART),
    ("stack", sym::STACK),
    ("stack_group", sym::STACK_GROUP),
    ("stack_hexagon", sym::STACK_HEXAGON),
    ("stack_off", sym::STACK_OFF),
    ("stack_star", sym::STACK_STAR),
    ("stacked_bar_chart", sym::STACKED_BAR_CHART),
    ("stacked_email", sym::STACKED_EMAIL),
    ("stacked_inbox", sym::STACKED_INBOX),
    ("stacked_line_chart", sym::STACKED_LINE_CHART),
    ("stacks", sym::STACKS),
    ("stadia_controller", sym::STADIA_CONTROLLER),
    ("stadium", sym::STADIUM),
    ("stairs", sym::STAIRS),
    ("stairs_2", sym::STAIRS_2),
    ("star", sym::STAR),
    ("star_border", sym::STAR_BORDER),
    ("star_border_purple500", sym::STAR_BORDER_PURPLE500),
    ("star_half", sym::STAR_HALF),
    ("star_outline", sym::STAR_OUTLINE),
    ("star_purple500", sym::STAR_PURPLE500),
    ("star_rate", sym::STAR_RATE),
    ("star_rate_half", sym::STAR_RATE_HALF),
    ("star_shine", sym::STAR_SHINE),
    ("stars", sym::STARS),
    ("stars_2", sym::STARS_2),
    ("start", sym::START),
    ("stat_0", sym::STAT_0),
    ("stat_1", sym::STAT_1),
    ("stat_2", sym::STAT_2),
    ("stat_3", sym::STAT_3),
    ("stat_minus_1", sym::STAT_MINUS_1),
    ("stat_minus_2", sym::STAT_MINUS_2),
    ("stat_minus_3", sym::STAT_MINUS_3),
    ("stay_current_landscape", sym::STAY_CURRENT_LANDSCAPE),
    ("stay_current_portrait", sym::STAY_CURRENT_PORTRAIT),
    ("stay_primary_landscape", sym::STAY_PRIMARY_LANDSCAPE),
    ("stay_primary_portrait", sym::STAY_PRIMARY_PORTRAIT),
    ("steering_wheel_cool", sym::STEERING_WHEEL_COOL),
    ("steering_wheel_heat", sym::STEERING_WHEEL_HEAT),
    ("step", sym::STEP),
    ("step_into", sym::STEP_INTO),
    ("step_out", sym::STEP_OUT),
    ("step_over", sym::STEP_OVER),
    ("steppers", sym::STEPPERS),
    ("steps", sym::STEPS),
    ("stethoscope", sym::STETHOSCOPE),
    ("stethoscope_arrow", sym::STETHOSCOPE_ARROW),
    ("stethoscope_check", sym::STETHOSCOPE_CHECK),
    ("sticker", sym::STICKER),
    ("sticker_add", sym::STICKER_ADD),
    ("sticky_note", sym::STICKY_NOTE),
    ("sticky_note_2", sym::STICKY_NOTE_2),
    ("stock_media", sym::STOCK_MEDIA),
    ("stockpot", sym::STOCKPOT),
    ("stop", sym::STOP),
    ("stop_circle", sym::STOP_CIRCLE),
    ("stop_screen_share", sym::STOP_SCREEN_SHARE),
    ("storage", sym::STORAGE),
    ("store", sym::STORE),
    ("store_mall_directory", sym::STORE_MALL_DIRECTORY),
    ("storefront", sym::STOREFRONT),
    ("storm", sym::STORM),
    ("straight", sym::STRAIGHT),
    ("straighten", sym::STRAIGHTEN),
    ("strategy", sym::STRATEGY),
    ("stream", sym::STREAM),
    ("stream_apps", sym::STREAM_APPS),
    ("streetview", sym::STREETVIEW),
    ("stress_management", sym::STRESS_MANAGEMENT),
    ("strikethrough_s", sym::STRIKETHROUGH_S),
    ("stroke_full", sym::STROKE_FULL),
    ("stroke_partial", sym::STROKE_PARTIAL),
    ("stroller", sym::STROLLER),
    ("style", sym::STYLE),
    ("styler", sym::STYLER),
    ("stylus", sym::STYLUS),
    ("stylus_brush", sym::STYLUS_BRUSH),
    ("stylus_fountain_pen", sym::STYLUS_FOUNTAIN_PEN),
    ("stylus_highlighter", sym::STYLUS_HIGHLIGHTER),
    ("stylus_laser_pointer", sym::STYLUS_LASER_POINTER),
    ("stylus_note", sym::STYLUS_NOTE),
    ("stylus_pen", sym::STYLUS_PEN),
    ("stylus_pencil", sym::STYLUS_PENCIL),
    ("subdirectory_arrow_left", sym::SUBDIRECTORY_ARROW_LEFT),
    ("subdirectory_arrow_right", sym::SUBDIRECTORY_ARROW_RIGHT),
    ("subheader", sym::SUBHEADER),
    ("subject", sym::SUBJECT),
    ("subscript", sym::SUBSCRIPT),
    ("subscriptions", sym::SUBSCRIPTIONS),
    ("subtitles", sym::SUBTITLES),
    ("subtitles_gear", sym::SUBTITLES_GEAR),
    ("subtitles_off", sym::SUBTITLES_OFF),
    ("subway", sym::SUBWAY),
    ("subway_walk", sym::SUBWAY_WALK),
    ("subwoofer", sym::SUBWOOFER),
    ("summarize", sym::SUMMARIZE),
    ("sunny", sym::SUNNY),
    ("sunny_snowing", sym::SUNNY_SNOWING),
    ("superscript", sym::SUPERSCRIPT),
    ("supervised_user_circle", sym::SUPERVISED_USER_CIRCLE),
    ("supervised_user_circle_off", sym::SUPERVISED_USER_CIRCLE_OFF),
    ("supervisor_account", sym::SUPERVISOR_ACCOUNT),
    ("support", sym::SUPPORT),
    ("support_agent", sym::SUPPORT_AGENT),
    ("surfing", sym::SURFING),
    ("surgical", sym::SURGICAL),
    ("surround_sound", sym::SURROUND_SOUND),
    ("swap_calls", sym::SWAP_CALLS),
    ("swap_driving_apps", sym::SWAP_DRIVING_APPS),
    ("swap_driving_apps_wheel", sym::SWAP_DRIVING_APPS_WHEEL),
    ("swap_horiz", sym::SWAP_HORIZ),
    ("swap_horizontal_circle", sym::SWAP_HORIZONTAL_CIRCLE),
    ("swap_vert", sym::SWAP_VERT),
    ("swap_vertical_circle", sym::SWAP_VERTICAL_CIRCLE),
    ("sweep", sym::SWEEP),
    ("swipe", sym::SWIPE),
    ("swipe_down", sym::SWIPE_DOWN),
    ("swipe_down_alt", sym::SWIPE_DOWN_ALT),
    ("swipe_left", sym::SWIPE_LEFT),
    ("swipe_left_2", sym::SWIPE_LEFT_2),
    ("swipe_left_alt", sym::SWIPE_LEFT_ALT),
    ("swipe_right", sym::SWIPE_RIGHT),
    ("swipe_right_2", sym::SWIPE_RIGHT_2),
    ("swipe_right_alt", sym::SWIPE_RIGHT_ALT),
    ("swipe_up", sym::SWIPE_UP),
    ("swipe_up_alt", sym::SWIPE_UP_ALT),
    ("swipe_vertical", sym::SWIPE_VERTICAL),
    ("switch", sym::SWITCH),
    ("switch_access", sym::SWITCH_ACCESS),
    ("switch_access_2", sym::SWITCH_ACCESS_2),
    ("switch_access_3", sym::SWITCH_ACCESS_3),
    ("switch_access_shortcut", sym::SWITCH_ACCESS_SHORTCUT),
    ("switch_access_shortcut_add", sym::SWITCH_ACCESS_SHORTCUT_ADD),
    ("switch_account", sym::SWITCH_ACCOUNT),
    ("switch_camera", sym::SWITCH_CAMERA),
    ("switch_left", sym::SWITCH_LEFT),
    ("switch_off", sym::SWITCH_OFF),
    ("switch_right", sym::SWITCH_RIGHT),
    ("switch_video", sym::SWITCH_VIDEO),
    ("switches", sym::SWITCHES),
    ("sword_rose", sym::SWORD_ROSE),
    ("swords", sym::SWORDS),
    ("symptoms", sym::SYMPTOMS),
    ("synagogue", sym::SYNAGOGUE),
    ("sync", sym::SYNC),
    ("sync_alt", sym::SYNC_ALT),
    ("sync_arrow_down", sym::SYNC_ARROW_DOWN),
    ("sync_arrow_up", sym::SYNC_ARROW_UP),
    ("sync_desktop", sym::SYNC_DESKTOP),
    ("sync_disabled", sym::SYNC_DISABLED),
    ("sync_lock", sym::SYNC_LOCK),
    ("sync_problem", sym::SYNC_PROBLEM),
    ("sync_saved_locally", sym::SYNC_SAVED_LOCALLY),
    ("sync_saved_locally_off", sym::SYNC_SAVED_LOCALLY_OFF),
    ("syringe", sym::SYRINGE),
    ("system_security_update", sym::SYSTEM_SECURITY_UPDATE),
    ("system_security_update_good", sym::SYSTEM_SECURITY_UPDATE_GOOD),
    ("system_security_update_warning", sym::SYSTEM_SECURITY_UPDATE_WARNING),
    ("system_update", sym::SYSTEM_UPDATE),
    ("system_update_alt", sym::SYSTEM_UPDATE_ALT),
    ("tab", sym::TAB),
    ("tab_close", sym::TAB_CLOSE),
    ("tab_close_inactive", sym::TAB_CLOSE_INACTIVE),
    ("tab_close_right", sym::TAB_CLOSE_RIGHT),
    ("tab_duplicate", sym::TAB_DUPLICATE),
    ("tab_group", sym::TAB_GROUP),
    ("tab_inactive", sym::TAB_INACTIVE),
    ("tab_move", sym::TAB_MOVE),
    ("tab_new_right", sym::TAB_NEW_RIGHT),
    ("tab_recent", sym::TAB_RECENT),
    ("tab_search", sym::TAB_SEARCH),
    ("tab_unselected", sym::TAB_UNSELECTED),
    ("table", sym::TABLE),
    ("table_bar", sym::TABLE_BAR),
    ("table_chart", sym::TABLE_CHART),
    ("table_chart_view", sym::TABLE_CHART_VIEW),
    ("table_convert", sym::TABLE_CONVERT),
    ("table_edit", sym::TABLE_EDIT),
    ("table_eye", sym::TABLE_EYE),
    ("table_lamp", sym::TABLE_LAMP),
    ("table_large", sym::TABLE_LARGE),
    ("table_restaurant", sym::TABLE_RESTAURANT),
    ("table_rows", sym::TABLE_ROWS),
    ("table_rows_narrow", sym::TABLE_ROWS_NARROW),
    ("table_sign", sym::TABLE_SIGN),
    ("table_view", sym::TABLE_VIEW),
    ("tablet", sym::TABLET),
    ("tablet_android", sym::TABLET_ANDROID),
    ("tablet_camera", sym::TABLET_CAMERA),
    ("tablet_mac", sym::TABLET_MAC),
    ("tabs", sym::TABS),
    ("tactic", sym::TACTIC),
    ("tag", sym::TAG),
    ("tag_faces", sym::TAG_FACES),
    ("takeout_dining", sym::TAKEOUT_DINING),
    ("takeout_dining_2", sym::TAKEOUT_DINING_2),
    ("tamper_detection_off", sym::TAMPER_DETECTION_OFF),
    ("tamper_detection_on", sym::TAMPER_DETECTION_ON),
    ("tap_and_play", sym::TAP_AND_PLAY),
    ("tapas", sym::TAPAS),
    ("target", sym::TARGET),
    ("target_check", sym::TARGET_CHECK),
    ("task", sym::TASK),
    ("task_alt", sym::TASK_ALT),
    ("tatami_seat", sym::TATAMI_SEAT),
    ("taunt", sym::TAUNT),
    ("taxi_alert", sym::TAXI_ALERT),
    ("team_dashboard", sym::TEAM_DASHBOARD),
    ("temp_preferences_custom", sym::TEMP_PREFERENCES_CUSTOM),
    ("temp_preferences_eco", sym::TEMP_PREFERENCES_ECO),
    ("temple_buddhist", sym::TEMPLE_BUDDHIST),
    ("temple_hindu", sym::TEMPLE_HINDU),
    ("tenancy", sym::TENANCY),
    ("terminal", sym::TERMINAL),
    ("terminal_2", sym::TERMINAL_2),
    ("terminal_add", sym::TERMINAL_ADD),
    ("terrain", sym::TERRAIN),
    ("text_ad", sym::TEXT_AD),
    ("text_ad_off", sym::TEXT_AD_OFF),
    ("text_compare", sym::TEXT_COMPARE),
    ("text_decrease", sym::TEXT_DECREASE),
    ("text_fields", sym::TEXT_FIELDS),
    ("text_fields_alt", sym::TEXT_FIELDS_ALT),
    ("text_format", sym::TEXT_FORMAT),
    ("text_increase", sym::TEXT_INCREASE),
    ("text_rotate_up", sym::TEXT_ROTATE_UP),
    ("text_rotate_vertical", sym::TEXT_ROTATE_VERTICAL),
    ("text_rotation_angledown", sym::TEXT_ROTATION_ANGLEDOWN),
    ("text_rotation_angleup", sym::TEXT_ROTATION_ANGLEUP),
    ("text_rotation_down", sym::TEXT_ROTATION_DOWN),
    ("text_rotation_none", sym::TEXT_ROTATION_NONE),
    ("text_select_end", sym::TEXT_SELECT_END),
    ("text_select_jump_to_beginning", sym::TEXT_SELECT_JUMP_TO_BEGINNING),
    ("text_select_jump_to_end", sym::TEXT_SELECT_JUMP_TO_END),
    ("text_select_move_back_character", sym::TEXT_SELECT_MOVE_BACK_CHARACTER),
    ("text_select_move_back_word", sym::TEXT_SELECT_MOVE_BACK_WORD),
    ("text_select_move_down", sym::TEXT_SELECT_MOVE_DOWN),
    ("text_select_move_forward_character", sym::TEXT_SELECT_MOVE_FORWARD_CHARACTER),
    ("text_select_move_forward_word", sym::TEXT_SELECT_MOVE_FORWARD_WORD),
    ("text_select_move_up", sym::TEXT_SELECT_MOVE_UP),
    ("text_select_start", sym::TEXT_SELECT_START),
    ("text_snippet", sym::TEXT_SNIPPET),
    ("text_to_speech", sym::TEXT_TO_SPEECH),
    ("text_up", sym::TEXT_UP),
    ("textsms", sym::TEXTSMS),
    ("texture", sym::TEXTURE),
    ("texture_add", sym::TEXTURE_ADD),
    ("texture_minus", sym::TEXTURE_MINUS),
    ("theater_comedy", sym::THEATER_COMEDY),
    ("theaters", sym::THEATERS),
    ("thermometer", sym::THERMOMETER),
    ("thermometer_add", sym::THERMOMETER_ADD),
    ("thermometer_alert", sym::THERMOMETER_ALERT),
    ("thermometer_gain", sym::THERMOMETER_GAIN),
    ("thermometer_loss", sym::THERMOMETER_LOSS),
    ("thermometer_minus", sym::THERMOMETER_MINUS),
    ("thermostat", sym::THERMOSTAT),
    ("thermostat_arrow_down", sym::THERMOSTAT_ARROW_DOWN),
    ("thermostat_arrow_up", sym::THERMOSTAT_ARROW_UP),
    ("thermostat_auto", sym::THERMOSTAT_AUTO),
    ("thermostat_carbon", sym::THERMOSTAT_CARBON),
    ("things_to_do", sym::THINGS_TO_DO),
    ("thread_unread", sym::THREAD_UNREAD),
    ("threat_intelligence", sym::THREAT_INTELLIGENCE),
    ("thumb_down", sym::THUMB_DOWN),
    ("thumb_down_alt", sym::THUMB_DOWN_ALT),
    ("thumb_down_filled", sym::THUMB_DOWN_FILLED),
    ("thumb_down_off", sym::THUMB_DOWN_OFF),
    ("thumb_down_off_alt", sym::THUMB_DOWN_OFF_ALT),
    ("thumb_up", sym::THUMB_UP),
    ("thumb_up_alt", sym::THUMB_UP_ALT),
    ("thumb_up_filled", sym::THUMB_UP_FILLED),
    ("thumb_up_off", sym::THUMB_UP_OFF),
    ("thumb_up_off_alt", sym::THUMB_UP_OFF_ALT),
    ("thumbnail_bar", sym::THUMBNAIL_BAR),
    ("thumbs_up_double", sym::THUMBS_UP_DOUBLE),
    ("thumbs_up_down", sym::THUMBS_UP_DOWN),
    ("thunderstorm", sym::THUNDERSTORM),
    ("tibia", sym::TIBIA),
    ("tibia_alt", sym::TIBIA_ALT),
    ("tile_large", sym::TILE_LARGE),
    ("tile_medium", sym::TILE_MEDIUM),
    ("tile_small", sym::TILE_SMALL),
    ("tilt_arrow_down", sym::TILT_ARROW_DOWN),
    ("tilt_arrow_up", sym::TILT_ARROW_UP),
    ("time_auto", sym::TIME_AUTO),
    ("time_to_leave", sym::TIME_TO_LEAVE),
    ("timelapse", sym::TIMELAPSE),
    ("timeline", sym::TIMELINE),
    ("timer", sym::TIMER),
    ("timer_1", sym::TIMER_1),
    ("timer_10", sym::TIMER_10),
    ("timer_10_alt_1", sym::TIMER_10_ALT_1),
    ("timer_10_select", sym::TIMER_10_SELECT),
    ("timer_2", sym::TIMER_2),
    ("timer_3", sym::TIMER_3),
    ("timer_3_alt_1", sym::TIMER_3_ALT_1),
    ("timer_3_select", sym::TIMER_3_SELECT),
    ("timer_5", sym::TIMER_5),
    ("timer_5_shutter", sym::TIMER_5_SHUTTER),
    ("timer_arrow_down", sym::TIMER_ARROW_DOWN),
    ("timer_arrow_up", sym::TIMER_ARROW_UP),
    ("timer_off", sym::TIMER_OFF),
    ("timer_pause", sym::TIMER_PAUSE),
    ("timer_play", sym::TIMER_PLAY),
    ("tips_and_updates", sym::TIPS_AND_UPDATES),
    ("tire_repair", sym::TIRE_REPAIR),
    ("title", sym::TITLE),
    ("titlecase", sym::TITLECASE),
    ("toast", sym::TOAST),
    ("toc", sym::TOC),
    ("today", sym::TODAY),
    ("toggle_off", sym::TOGGLE_OFF),
    ("toggle_on", sym::TOGGLE_ON),
    ("token", sym::TOKEN),
    ("toll", sym::TOLL),
    ("tonality", sym::TONALITY),
    ("tonality_2", sym::TONALITY_2),
    ("toolbar", sym::TOOLBAR),
    ("tools_flat_head", sym::TOOLS_FLAT_HEAD),
    ("tools_installation_kit", sym::TOOLS_INSTALLATION_KIT),
    ("tools_ladder", sym::TOOLS_LADDER),
    ("tools_level", sym::TOOLS_LEVEL),
    ("tools_phillips", sym::TOOLS_PHILLIPS),
    ("tools_pliers_wire_stripper", sym::TOOLS_PLIERS_WIRE_STRIPPER),
    ("tools_power_drill", sym::TOOLS_POWER_DRILL),
    ("tools_wrench", sym::TOOLS_WRENCH),
    ("tooltip", sym::TOOLTIP),
    ("tooltip_2", sym::TOOLTIP_2),
    ("top_panel_close", sym::TOP_PANEL_CLOSE),
    ("top_panel_open", sym::TOP_PANEL_OPEN),
    ("topic", sym::TOPIC),
    ("tornado", sym::TORNADO),
    ("total_dissolved_solids", sym::TOTAL_DISSOLVED_SOLIDS),
    ("touch_app", sym::TOUCH_APP),
    ("touch_double", sym::TOUCH_DOUBLE),
    ("touch_double_2", sym::TOUCH_DOUBLE_2),
    ("touch_long", sym::TOUCH_LONG),
    ("touch_triple", sym::TOUCH_TRIPLE),
    ("touchpad_mouse", sym::TOUCHPAD_MOUSE),
    ("touchpad_mouse_off", sym::TOUCHPAD_MOUSE_OFF),
    ("tour", sym::TOUR),
    ("toys", sym::TOYS),
    ("toys_and_games", sym::TOYS_AND_GAMES),
    ("toys_fan", sym::TOYS_FAN),
    ("track_changes", sym::TRACK_CHANGES),
    ("trackpad_input", sym::TRACKPAD_INPUT),
    ("trackpad_input_2", sym::TRACKPAD_INPUT_2),
    ("trackpad_input_3", sym::TRACKPAD_INPUT_3),
    ("traffic", sym::TRAFFIC),
    ("traffic_jam", sym::TRAFFIC_JAM),
    ("trail_length", sym::TRAIL_LENGTH),
    ("trail_length_medium", sym::TRAIL_LENGTH_MEDIUM),
    ("trail_length_short", sym::TRAIL_LENGTH_SHORT),
    ("train", sym::TRAIN),
    ("tram", sym::TRAM),
    ("transcribe", sym::TRANSCRIBE),
    ("transfer_within_a_station", sym::TRANSFER_WITHIN_A_STATION),
    ("transform", sym::TRANSFORM),
    ("transgender", sym::TRANSGENDER),
    ("transit_enterexit", sym::TRANSIT_ENTEREXIT),
    ("transit_ticket", sym::TRANSIT_TICKET),
    ("transition_chop", sym::TRANSITION_CHOP),
    ("transition_dissolve", sym::TRANSITION_DISSOLVE),
    ("transition_fade", sym::TRANSITION_FADE),
    ("transition_push", sym::TRANSITION_PUSH),
    ("transition_slide", sym::TRANSITION_SLIDE),
    ("translate", sym::TRANSLATE),
    ("translate_indic", sym::TRANSLATE_INDIC),
    ("transportation", sym::TRANSPORTATION),
    ("travel", sym::TRAVEL),
    ("travel_explore", sym::TRAVEL_EXPLORE),
    ("travel_luggage_and_bags", sym::TRAVEL_LUGGAGE_AND_BAGS),
    ("trending_down", sym::TRENDING_DOWN),
    ("trending_flat", sym::TRENDING_FLAT),
    ("trending_up", sym::TRENDING_UP),
    ("triangle_circle", sym::TRIANGLE_CIRCLE),
    ("trip", sym::TRIP),
    ("trip_origin", sym::TRIP_ORIGIN),
    ("trolley", sym::TROLLEY),
    ("trolley_cable_car", sym::TROLLEY_CABLE_CAR),
    ("trophy", sym::TROPHY),
    ("troubleshoot", sym::TROUBLESHOOT),
    ("try", sym::TRY),
    ("tsunami", sym::TSUNAMI),
    ("tsv", sym::TSV),
    ("tty", sym::TTY),
    ("tune", sym::TUNE),
    ("tungsten", sym::TUNGSTEN),
    ("turn_left", sym::TURN_LEFT),
    ("turn_right", sym::TURN_RIGHT),
    ("turn_sharp_left", sym::TURN_SHARP_LEFT),
    ("turn_sharp_right", sym::TURN_SHARP_RIGHT),
    ("turn_slight_left", sym::TURN_SLIGHT_LEFT),
    ("turn_slight_right", sym::TURN_SLIGHT_RIGHT),
    ("turned_in", sym::TURNED_IN),
    ("turned_in_not", sym::TURNED_IN_NOT),
    ("tv", sym::TV),
    ("tv_displays", sym::TV_DISPLAYS),
    ("tv_gen", sym::TV_GEN),
    ("tv_guide", sym::TV_GUIDE),
    ("tv_next", sym::TV_NEXT),
    ("tv_off", sym::TV_OFF),
    ("tv_options_edit_channels", sym::TV_OPTIONS_EDIT_CHANNELS),
    ("tv_options_input_settings", sym::TV_OPTIONS_INPUT_SETTINGS),
    ("tv_remote", sym::TV_REMOTE),
    ("tv_signin", sym::TV_SIGNIN),
    ("tv_with_assistant", sym::TV_WITH_ASSISTANT),
    ("two_pager", sym::TWO_PAGER),
    ("two_pager_store", sym::TWO_PAGER_STORE),
    ("two_wheeler", sym::TWO_WHEELER),
    ("type_specimen", sym::TYPE_SPECIMEN),
    ("u_turn_left", sym::U_TURN_LEFT),
    ("u_turn_right", sym::U_TURN_RIGHT),
    ("udon", sym::UDON),
    ("ulna_radius", sym::ULNA_RADIUS),
    ("ulna_radius_alt", sym::ULNA_RADIUS_ALT),
    ("umbrella", sym::UMBRELLA),
    ("unarchive", sym::UNARCHIVE),
    ("undereye", sym::UNDEREYE),
    ("undo", sym::UNDO),
    ("unfold_less", sym::UNFOLD_LESS),
    ("unfold_less_double", sym::UNFOLD_LESS_DOUBLE),
    ("unfold_more", sym::UNFOLD_MORE),
    ("unfold_more_double", sym::UNFOLD_MORE_DOUBLE),
    ("ungroup", sym::UNGROUP),
    ("universal_currency", sym::UNIVERSAL_CURRENCY),
    ("universal_currency_alt", sym::UNIVERSAL_CURRENCY_ALT),
    ("universal_local", sym::UNIVERSAL_LOCAL),
    ("unknown_2", sym::UNKNOWN_2),
    ("unknown_5", sym::UNKNOWN_5),
    ("unknown_7", sym::UNKNOWN_7),
    ("unknown_document", sym::UNKNOWN_DOCUMENT),
    ("unknown_med", sym::UNKNOWN_MED),
    ("unlicense", sym::UNLICENSE),
    ("unpaved_road", sym::UNPAVED_ROAD),
    ("unpin", sym::UNPIN),
    ("unpublished", sym::UNPUBLISHED),
    ("unsubscribe", sym::UNSUBSCRIBE),
    ("upcoming", sym::UPCOMING),
    ("update", sym::UPDATE),
    ("update_disabled", sym::UPDATE_DISABLED),
    ("upgrade", sym::UPGRADE),
    ("upi_pay", sym::UPI_PAY),
    ("upload", sym::UPLOAD),
    ("upload_2", sym::UPLOAD_2),
    ("upload_file", sym::UPLOAD_FILE),
    ("uppercase", sym::UPPERCASE),
    ("urology", sym::UROLOGY),
    ("usb", sym::USB),
    ("usb_off", sym::USB_OFF),
    ("user_attributes", sym::USER_ATTRIBUTES),
    ("vaccines", sym::VACCINES),
    ("vacuum", sym::VACUUM),
    ("vacuum_2", sym::VACUUM_2),
    ("vacuum_2_on", sym::VACUUM_2_ON),
    ("valve", sym::VALVE),
    ("vape_free", sym::VAPE_FREE),
    ("vaping_rooms", sym::VAPING_ROOMS),
    ("variable_add", sym::VARIABLE_ADD),
    ("variable_insert", sym::VARIABLE_INSERT),
    ("variable_remove", sym::VARIABLE_REMOVE),
    ("variables", sym::VARIABLES),
    ("ventilator", sym::VENTILATOR),
    ("verified", sym::VERIFIED),
    ("verified_off", sym::VERIFIED_OFF),
    ("verified_user", sym::VERIFIED_USER),
    ("vertical_align_bottom", sym::VERTICAL_ALIGN_BOTTOM),
    ("vertical_align_center", sym::VERTICAL_ALIGN_CENTER),
    ("vertical_align_top", sym::VERTICAL_ALIGN_TOP),
    ("vertical_distribute", sym::VERTICAL_DISTRIBUTE),
    ("vertical_shades", sym::VERTICAL_SHADES),
    ("vertical_shades_closed", sym::VERTICAL_SHADES_CLOSED),
    ("vertical_split", sym::VERTICAL_SPLIT),
    ("vibration", sym::VIBRATION),
    ("video_call", sym::VIDEO_CALL),
    ("video_camera_back", sym::VIDEO_CAMERA_BACK),
    ("video_camera_back_add", sym::VIDEO_CAMERA_BACK_ADD),
    ("video_camera_front", sym::VIDEO_CAMERA_FRONT),
    ("video_camera_front_off", sym::VIDEO_CAMERA_FRONT_OFF),
    ("video_chat", sym::VIDEO_CHAT),
    ("video_file", sym::VIDEO_FILE),
    ("video_frame_copy", sym::VIDEO_FRAME_COPY),
    ("video_frame_save", sym::VIDEO_FRAME_SAVE),
    ("video_label", sym::VIDEO_LABEL),
    ("video_library", sym::VIDEO_LIBRARY),
    ("video_search", sym::VIDEO_SEARCH),
    ("video_settings", sym::VIDEO_SETTINGS),
    ("video_stable", sym::VIDEO_STABLE),
    ("video_template", sym::VIDEO_TEMPLATE),
    ("videocam", sym::VIDEOCAM),
    ("videocam_alert", sym::VIDEOCAM_ALERT),
    ("videocam_off", sym::VIDEOCAM_OFF),
    ("videogame_asset", sym::VIDEOGAME_ASSET),
    ("videogame_asset_off", sym::VIDEOGAME_ASSET_OFF),
    ("view_agenda", sym::VIEW_AGENDA),
    ("view_apps", sym::VIEW_APPS),
    ("view_array", sym::VIEW_ARRAY),
    ("view_carousel", sym::VIEW_CAROUSEL),
    ("view_column", sym::VIEW_COLUMN),
    ("view_column_2", sym::VIEW_COLUMN_2),
    ("view_comfy", sym::VIEW_COMFY),
    ("view_comfy_alt", sym::VIEW_COMFY_ALT),
    ("view_compact", sym::VIEW_COMPACT),
    ("view_compact_alt", sym::VIEW_COMPACT_ALT),
    ("view_cozy", sym::VIEW_COZY),
    ("view_day", sym::VIEW_DAY),
    ("view_headline", sym::VIEW_HEADLINE),
    ("view_in_ar", sym::VIEW_IN_AR),
    ("view_in_ar_new", sym::VIEW_IN_AR_NEW),
    ("view_in_ar_off", sym::VIEW_IN_AR_OFF),
    ("view_kanban", sym::VIEW_KANBAN),
    ("view_list", sym::VIEW_LIST),
    ("view_module", sym::VIEW_MODULE),
    ("view_object_track", sym::VIEW_OBJECT_TRACK),
    ("view_quilt", sym::VIEW_QUILT),
    ("view_real_size", sym::VIEW_REAL_SIZE),
    ("view_sidebar", sym::VIEW_SIDEBAR),
    ("view_stream", sym::VIEW_STREAM),
    ("view_timeline", sym::VIEW_TIMELINE),
    ("view_week", sym::VIEW_WEEK),
    ("vignette", sym::VIGNETTE),
    ("vignette_2", sym::VIGNETTE_2),
    ("villa", sym::VILLA),
    ("visibility", sym::VISIBILITY),
    ("visibility_lock", sym::VISIBILITY_LOCK),
    ("visibility_off", sym::VISIBILITY_OFF),
    ("vital_signs", sym::VITAL_SIGNS),
    ("vitals", sym::VITALS),
    ("vo2_max", sym::VO2_MAX),
    ("voice_chat", sym::VOICE_CHAT),
    ("voice_chat_off", sym::VOICE_CHAT_OFF),
    ("voice_over_off", sym::VOICE_OVER_OFF),
    ("voice_selection", sym::VOICE_SELECTION),
    ("voice_selection_off", sym::VOICE_SELECTION_OFF),
    ("voicemail", sym::VOICEMAIL),
    ("voicemail_2", sym::VOICEMAIL_2),
    ("volcano", sym::VOLCANO),
    ("volume_down", sym::VOLUME_DOWN),
    ("volume_down_alt", sym::VOLUME_DOWN_ALT),
    ("volume_mute", sym::VOLUME_MUTE),
    ("volume_off", sym::VOLUME_OFF),
    ("volume_up", sym::VOLUME_UP),
    ("volunteer_activism", sym::VOLUNTEER_ACTIVISM),
    ("voting_chip", sym::VOTING_CHIP),
    ("vpn_key", sym::VPN_KEY),
    ("vpn_key_alert", sym::VPN_KEY_ALERT),
    ("vpn_key_off", sym::VPN_KEY_OFF),
    ("vpn_lock", sym::VPN_LOCK),
    ("vpn_lock_2", sym::VPN_LOCK_2),
    ("vr180_create2d", sym::VR180_CREATE2D),
    ("vr180_create2d_off", sym::VR180_CREATE2D_OFF),
    ("vrpano", sym::VRPANO),
    ("walk_bike", sym::WALK_BIKE),
    ("wall_art", sym::WALL_ART),
    ("wall_lamp", sym::WALL_LAMP),
    ("wallet", sym::WALLET),
    ("wallpaper", sym::WALLPAPER),
    ("wallpaper_slideshow", sym::WALLPAPER_SLIDESHOW),
    ("wand_shine", sym::WAND_SHINE),
    ("wand_stars", sym::WAND_STARS),
    ("ward", sym::WARD),
    ("warehouse", sym::WAREHOUSE),
    ("warning", sym::WARNING),
    ("warning_amber", sym::WARNING_AMBER),
    ("warning_off", sym::WARNING_OFF),
    ("wash", sym::WASH),
    ("washoku", sym::WASHOKU),
    ("watch", sym::WATCH),
    ("watch_alert", sym::WATCH_ALERT),
    ("watch_arrow", sym::WATCH_ARROW),
    ("watch_arrow_down", sym::WATCH_ARROW_DOWN),
    ("watch_button", sym::WATCH_BUTTON),
    ("watch_button_press", sym::WATCH_BUTTON_PRESS),
    ("watch_check", sym::WATCH_CHECK),
    ("watch_later", sym::WATCH_LATER),
    ("watch_lock", sym::WATCH_LOCK),
    ("watch_off", sym::WATCH_OFF),
    ("watch_screentime", sym::WATCH_SCREENTIME),
    ("watch_vibration", sym::WATCH_VIBRATION),
    ("watch_wake", sym::WATCH_WAKE),
    ("water", sym::WATER),
    ("water_bottle", sym::WATER_BOTTLE),
    ("water_bottle_large", sym::WATER_BOTTLE_LARGE),
    ("water_damage", sym::WATER_DAMAGE),
    ("water_do", sym::WATER_DO),
    ("water_drop", sym::WATER_DROP),
    ("water_drops", sym::WATER_DROPS),
    ("water_ec", sym::WATER_EC),
    ("water_full", sym::WATER_FULL),
    ("water_heater", sym::WATER_HEATER),
    ("water_lock", sym::WATER_LOCK),
    ("water_loss", sym::WATER_LOSS),
    ("water_lux", sym::WATER_LUX),
    ("water_medium", sym::WATER_MEDIUM),
    ("water_orp", sym::WATER_ORP),
    ("water_ph", sym::WATER_PH),
    ("water_pump", sym::WATER_PUMP),
    ("water_voc", sym::WATER_VOC),
    ("waterfall_chart", sym::WATERFALL_CHART),
    ("waves", sym::WAVES),
    ("waving_hand", sym::WAVING_HAND),
    ("wb_auto", sym::WB_AUTO),
    ("wb_cloudy", sym::WB_CLOUDY),
    ("wb_incandescent", sym::WB_INCANDESCENT),
    ("wb_iridescent", sym::WB_IRIDESCENT),
    ("wb_shade", sym::WB_SHADE),
    ("wb_sunny", sym::WB_SUNNY),
    ("wb_twilight", sym::WB_TWILIGHT),
    ("wb_twilight_2", sym::WB_TWILIGHT_2),
    ("wc", sym::WC),
    ("weather_hail", sym::WEATHER_HAIL),
    ("weather_mix", sym::WEATHER_MIX),
    ("weather_snowy", sym::WEATHER_SNOWY),
    ("web", sym::WEB),
    ("web_asset", sym::WEB_ASSET),
    ("web_asset_off", sym::WEB_ASSET_OFF),
    ("web_stories", sym::WEB_STORIES),
    ("web_traffic", sym::WEB_TRAFFIC),
    ("webhook", sym::WEBHOOK),
    ("weekend", sym::WEEKEND),
    ("weight", sym::WEIGHT),
    ("west", sym::WEST),
    ("whatshot", sym::WHATSHOT),
    ("wheat", sym::WHEAT),
    ("wheelchair_pickup", sym::WHEELCHAIR_PICKUP),
    ("where_to_vote", sym::WHERE_TO_VOTE),
    ("widget_medium", sym::WIDGET_MEDIUM),
    ("widget_menu", sym::WIDGET_MENU),
    ("widget_small", sym::WIDGET_SMALL),
    ("widget_width", sym::WIDGET_WIDTH),
    ("widgets", sym::WIDGETS),
    ("width", sym::WIDTH),
    ("width_full", sym::WIDTH_FULL),
    ("width_normal", sym::WIDTH_NORMAL),
    ("width_wide", sym::WIDTH_WIDE),
    ("wifi", sym::WIFI),
    ("wifi_1_bar", sym::WIFI_1_BAR),
    ("wifi_2_bar", sym::WIFI_2_BAR),
    ("wifi_add", sym::WIFI_ADD),
    ("wifi_calling", sym::WIFI_CALLING),
    ("wifi_calling_1", sym::WIFI_CALLING_1),
    ("wifi_calling_2", sym::WIFI_CALLING_2),
    ("wifi_calling_3", sym::WIFI_CALLING_3),
    ("wifi_calling_bar_1", sym::WIFI_CALLING_BAR_1),
    ("wifi_calling_bar_2", sym::WIFI_CALLING_BAR_2),
    ("wifi_calling_bar_3", sym::WIFI_CALLING_BAR_3),
    ("wifi_channel", sym::WIFI_CHANNEL),
    ("wifi_device", sym::WIFI_DEVICE),
    ("wifi_find", sym::WIFI_FIND),
    ("wifi_home", sym::WIFI_HOME),
    ("wifi_lock", sym::WIFI_LOCK),
    ("wifi_notification", sym::WIFI_NOTIFICATION),
    ("wifi_off", sym::WIFI_OFF),
    ("wifi_password", sym::WIFI_PASSWORD),
    ("wifi_protected_setup", sym::WIFI_PROTECTED_SETUP),
    ("wifi_proxy", sym::WIFI_PROXY),
    ("wifi_tethering", sym::WIFI_TETHERING),
    ("wifi_tethering_error", sym::WIFI_TETHERING_ERROR),
    ("wifi_tethering_off", sym::WIFI_TETHERING_OFF),
    ("wind_power", sym::WIND_POWER),
    ("window", sym::WINDOW),
    ("window_closed", sym::WINDOW_CLOSED),
    ("window_open", sym::WINDOW_OPEN),
    ("window_sensor", sym::WINDOW_SENSOR),
    ("windshield_defrost_auto", sym::WINDSHIELD_DEFROST_AUTO),
    ("windshield_defrost_front", sym::WINDSHIELD_DEFROST_FRONT),
    ("windshield_defrost_rear", sym::WINDSHIELD_DEFROST_REAR),
    ("windshield_heat_front", sym::WINDSHIELD_HEAT_FRONT),
    ("wine_bar", sym::WINE_BAR),
    ("woman", sym::WOMAN),
    ("woman_2", sym::WOMAN_2),
    ("work", sym::WORK),
    ("work_alert", sym::WORK_ALERT),
    ("work_history", sym::WORK_HISTORY),
    ("work_off", sym::WORK_OFF),
    ("work_outline", sym::WORK_OUTLINE),
    ("work_update", sym::WORK_UPDATE),
    ("workflow", sym::WORKFLOW),
    ("workspace_premium", sym::WORKSPACE_PREMIUM),
    ("workspaces", sym::WORKSPACES),
    ("workspaces_outline", sym::WORKSPACES_OUTLINE),
    ("wounds_injuries", sym::WOUNDS_INJURIES),
    ("wrap_text", sym::WRAP_TEXT),
    ("wrist", sym::WRIST),
    ("wrong_location", sym::WRONG_LOCATION),
    ("wysiwyg", sym::WYSIWYG),
    ("x_circle", sym::X_CIRCLE),
    ("y_circle", sym::Y_CIRCLE),
    ("yakitori", sym::YAKITORI),
    ("yard", sym::YARD),
    ("yoshoku", sym::YOSHOKU),
    ("your_trips", sym::YOUR_TRIPS),
    ("youtube_activity", sym::YOUTUBE_ACTIVITY),
    ("youtube_searched_for", sym::YOUTUBE_SEARCHED_FOR),
    ("zone_person_alert", sym::ZONE_PERSON_ALERT),
    ("zone_person_idle", sym::ZONE_PERSON_IDLE),
    ("zone_person_urgent", sym::ZONE_PERSON_URGENT),
    ("zoom_in", sym::ZOOM_IN),
    ("zoom_in_map", sym::ZOOM_IN_MAP),
    ("zoom_out", sym::ZOOM_OUT),
    ("zoom_out_map", sym::ZOOM_OUT_MAP),
];
