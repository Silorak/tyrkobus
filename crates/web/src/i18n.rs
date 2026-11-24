//! Interface text in Slovak, Hungarian and English. Stop and line names stay
//! as the operator prints them.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Lang {
    Sk,
    Hu,
    En,
}

impl Lang {
    pub const ALL: [Lang; 3] = [Lang::Sk, Lang::Hu, Lang::En];

    pub fn code(self) -> &'static str {
        match self {
            Lang::Sk => "sk",
            Lang::Hu => "hu",
            Lang::En => "en",
        }
    }

    pub fn from_code(code: &str) -> Option<Lang> {
        Lang::ALL.into_iter().find(|l| l.code() == code)
    }
}

pub enum Text {
    Title,
    SearchLabel,
    SearchButton,
    SearchHint,
    NoResults,
    BusiestStops,
    Departures,
    NoDepartures,
    Scheduled,
    Approximate,
    BoardMode,
    Tomorrow,
    NoMoreToday,
    Today,
    RequestStop,
    Towards,
    LinesHere,
    TimetableFor,
    Direction,
    AllStops,
    SourcePage,
    Unofficial,
    Now,
    Back,
    Offline,
    Trip,
    NoTrips,
    Arrival,
    OtherTime,
    Show,
    OnMap,
    DropOffOnly,
    BoardingOnly,
    From,
    To,
    FindBuses,
    Swap,
    NoJourney,
    UnknownStop,
    StopDepartures,
    Journeys,
    Details,
    MapOfJourney,
    WhereToday,
    Region,
    PhotoBy,
    LeaveNow,
    Recent,
    PlanFromHere,
    PlanToHere,
    Change,
    Language,
}

pub fn t(lang: Lang, text: Text) -> &'static str {
    use Lang::*;
    use Text::*;
    match (text, lang) {
        (Title, Sk) => "Odchody autobusov",
        (Title, Hu) => "Buszindulások",
        (Title, En) => "Bus departures",
        (SearchLabel, Sk) => "Zastávka",
        (SearchLabel, Hu) => "Megálló",
        (SearchLabel, En) => "Stop",
        (SearchButton, Sk) => "Hľadať",
        (SearchButton, Hu) => "Keresés",
        (SearchButton, En) => "Search",
        (SearchHint, Sk) => "napr. Komárno, AS",
        (SearchHint, Hu) => "pl. Komárno, AS",
        (SearchHint, En) => "e.g. Komárno, AS",
        (NoResults, Sk) => "Žiadna zastávka s týmto názvom. Skúste časť názvu obce.",
        (NoResults, Hu) => "Nincs ilyen nevű megálló. Próbálja a település nevének egy részét.",
        (NoResults, En) => "No stop with that name. Try part of the village name.",
        (BusiestStops, Sk) => "Najrušnejšie zastávky",
        (BusiestStops, Hu) => "Legforgalmasabb megállók",
        (BusiestStops, En) => "Busiest stops",
        (Departures, Sk) => "Najbližšie odchody",
        (Departures, Hu) => "Következő indulások",
        (Departures, En) => "Next departures",
        (NoDepartures, Sk) => "V najbližších dvoch dňoch odtiaľto nič nepremáva.",
        (NoDepartures, Hu) => "A következő két napban innen nem indul járat.",
        (NoDepartures, En) => "Nothing leaves from here in the next two days.",
        (Scheduled, Sk) => "Časy podľa cestovného poriadku, nie poloha autobusu naživo.",
        (Scheduled, Hu) => "Menetrend szerinti idők, nem élő járatkövetés.",
        (Scheduled, En) => "Timetabled times, not live bus positions.",
        (Approximate, Sk) => "Poloha zastávky je približná (stred obce).",
        (Approximate, Hu) => "A megálló helye csak hozzávetőleges (a település közepe).",
        (Approximate, En) => "Stop location is approximate (village centre).",
        (BoardMode, Sk) => "Tabuľa na obrazovku",
        (BoardMode, Hu) => "Kijelző mód",
        (BoardMode, En) => "Screen board",
        (Tomorrow, Sk) => "zajtra",
        (Tomorrow, Hu) => "holnap",
        (Tomorrow, En) => "tomorrow",
        (NoMoreToday, Sk) => "Dnes už žiadny spoj nejde.",
        (NoMoreToday, Hu) => "Ma már nem indul járat.",
        (NoMoreToday, En) => "No more buses today.",
        (Today, Sk) => "dnes",
        (Today, Hu) => "ma",
        (Today, En) => "today",
        (RequestStop, Sk) => "na znamenie",
        (RequestStop, Hu) => "feltételes megálló",
        (RequestStop, En) => "request stop",
        (Towards, Sk) => "smer",
        (Towards, Hu) => "irány",
        (Towards, En) => "to",
        (LinesHere, Sk) => "Linky na tejto zastávke",
        (LinesHere, Hu) => "Vonalak ezen a megállón",
        (LinesHere, En) => "Lines at this stop",
        (TimetableFor, Sk) => "Cestovný poriadok na",
        (TimetableFor, Hu) => "Menetrend erre a napra:",
        (TimetableFor, En) => "Timetable for",
        (Direction, Sk) => "Smer",
        (Direction, Hu) => "Irány",
        (Direction, En) => "Towards",
        (AllStops, Sk) => "Všetky zastávky spoja",
        (AllStops, Hu) => "A járat összes megállója",
        (AllStops, En) => "Every stop on this trip",
        (SourcePage, Sk) => "Zdroj: cestovný poriadok ARRIVA (PDF), strana",
        (SourcePage, Hu) => "Forrás: ARRIVA menetrend (PDF), oldal",
        (SourcePage, En) => "Source: ARRIVA timetable (PDF), page",
        (Unofficial, Sk) => {
            "Neoficiálna stránka. Údaje sú prevzaté z verejných cestovných poriadkov ARRIVA Nové Zámky."
        }
        (Unofficial, Hu) => {
            "Nem hivatalos oldal. Az adatok az ARRIVA Nové Zámky nyilvános menetrendjeiből származnak."
        }
        (Unofficial, En) => {
            "Unofficial site. Data comes from the public timetables of ARRIVA Nové Zámky."
        }
        (Now, Sk) => "teraz",
        (Now, Hu) => "most",
        (Now, En) => "now",
        (Back, Sk) => "Späť",
        (Back, Hu) => "Vissza",
        (Back, En) => "Back",
        (Offline, Sk) => "Bez pripojenia, zobrazujem posledné údaje",
        (Offline, Hu) => "Nincs kapcsolat, az utolsó adatokat mutatom",
        (Offline, En) => "Offline, showing the last update",
        (Trip, Sk) => "Spoj",
        (Trip, Hu) => "Járat",
        (Trip, En) => "Trip",
        (NoTrips, Sk) => "V tento deň linka nepremáva.",
        (NoTrips, Hu) => "Ezen a napon a vonal nem közlekedik.",
        (NoTrips, En) => "This line does not run on this day.",
        (Arrival, Sk) => "príchod",
        (Arrival, Hu) => "érkezés",
        (Arrival, En) => "arrives",
        (OtherTime, Sk) => "Iný deň alebo čas",
        (OtherTime, Hu) => "Más nap vagy időpont",
        (OtherTime, En) => "Another day or time",
        (Show, Sk) => "Zobraziť",
        (Show, Hu) => "Mutasd",
        (Show, En) => "Show",
        (OnMap, Sk) => "Na mape",
        (OnMap, Hu) => "Térképen",
        (OnMap, En) => "On the map",
        (DropOffOnly, Sk) => "len výstup",
        (DropOffOnly, Hu) => "csak leszállás",
        (DropOffOnly, En) => "drop-off only",
        (BoardingOnly, Sk) => "len nástup",
        (BoardingOnly, Hu) => "csak felszállás",
        (BoardingOnly, En) => "boarding only",
        (From, Sk) => "Odkiaľ",
        (From, Hu) => "Honnan",
        (From, En) => "From",
        (To, Sk) => "Kam",
        (To, Hu) => "Hová",
        (To, En) => "To",
        (FindBuses, Sk) => "Nájsť spoje",
        (FindBuses, Hu) => "Járatok keresése",
        (FindBuses, En) => "Find buses",
        (Swap, Sk) => "Vymeniť smer",
        (Swap, Hu) => "Irány csere",
        (Swap, En) => "Swap",
        (NoJourney, Sk) => {
            "V tomto čase sa tam autobusom nedostanete. Skúste iný čas alebo susednú zastávku."
        }
        (NoJourney, Hu) => {
            "Ebben az időben busszal nem jut el oda. Próbáljon más időpontot vagy szomszédos megállót."
        }
        (NoJourney, En) => {
            "No bus gets you there around this time. Try another time or a nearby stop."
        }
        (UnknownStop, Sk) => "Nepoznám zastávku",
        (UnknownStop, Hu) => "Ismeretlen megálló",
        (UnknownStop, En) => "Unknown stop",
        (StopDepartures, Sk) => "Odchody zo zastávky",
        (StopDepartures, Hu) => "Indulások egy megállóból",
        (StopDepartures, En) => "Departures from a stop",
        (Journeys, Sk) => "Spojenia",
        (Journeys, Hu) => "Útvonalak",
        (Journeys, En) => "Journeys",
        (Details, Sk) => "Podrobnosti spoja",
        (Details, Hu) => "Járat részletei",
        (Details, En) => "Trip details",
        (MapOfJourney, Sk) => "Mapa spojenia",
        (MapOfJourney, Hu) => "Az útvonal térképe",
        (MapOfJourney, En) => "Map of the journey",
        (WhereToday, Sk) => "Kam dnes?",
        (WhereToday, Hu) => "Hová ma?",
        (WhereToday, En) => "Where to today?",
        (Region, Sk) => "Autobusy okolo Nových Zámkov, Komárna, Levíc a Šale",
        (Region, Hu) => "Buszok Érsekújvár, Komárom, Léva és Vágsellye környékén",
        (Region, En) => "Buses around Nové Zámky, Komárno, Levice and Šaľa",
        (PhotoBy, Sk) => "Foto",
        (PhotoBy, Hu) => "Fotó",
        (PhotoBy, En) => "Photo",
        (LeaveNow, Sk) => "Teraz",
        (LeaveNow, Hu) => "Most",
        (LeaveNow, En) => "Now",
        (Recent, Sk) => "Nedávno hľadané",
        (Recent, Hu) => "Legutóbbi keresések",
        (Recent, En) => "Recent trips",
        (PlanFromHere, Sk) => "Cesta odtiaľto",
        (PlanFromHere, Hu) => "Útvonal innen",
        (PlanFromHere, En) => "Trip from here",
        (PlanToHere, Sk) => "Cesta sem",
        (PlanToHere, Hu) => "Útvonal ide",
        (PlanToHere, En) => "Trip to here",
        (Change, Sk) => "Zmeniť",
        (Change, Hu) => "Módosítás",
        (Change, En) => "Change",
        (Language, Sk) => "Jazyk",
        (Language, Hu) => "Nyelv",
        (Language, En) => "Language",
    }
}

/// Short weekday + date, e.g. "po 15. 12." / "H 12. 15." / "Mon 15 Dec".
pub fn day(lang: Lang, d: chrono::NaiveDate) -> String {
    use chrono::Datelike;
    let wd = d.weekday().num_days_from_monday() as usize;
    match lang {
        Lang::Sk => format!(
            "{} {}. {}.",
            ["po", "ut", "st", "št", "pi", "so", "ne"][wd],
            d.day(),
            d.month()
        ),
        Lang::Hu => format!(
            "{} {}. {}.",
            ["H", "K", "Sze", "Cs", "P", "Szo", "V"][wd],
            d.month(),
            d.day()
        ),
        Lang::En => format!(
            "{} {} {}",
            ["Mon", "Tue", "Wed", "Thu", "Fri", "Sat", "Sun"][wd],
            d.day(),
            d.format("%b")
        ),
    }
}

/// "priamy spoj" / "1 prestup" / "2 prestupy" / "5 prestupov".
pub fn changes(lang: Lang, n: usize) -> String {
    match (lang, n) {
        (Lang::Sk, 0) => "priamy spoj".into(),
        (Lang::Sk, 1) => "1 prestup".into(),
        (Lang::Sk, 2..=4) => format!("{n} prestupy"),
        (Lang::Sk, _) => format!("{n} prestupov"),
        (Lang::Hu, 0) => "közvetlen járat".into(),
        (Lang::Hu, _) => format!("{n} átszállás"),
        (Lang::En, 0) => "direct".into(),
        (Lang::En, 1) => "1 change".into(),
        (Lang::En, _) => format!("{n} changes"),
    }
}

/// "Pešo 5 min" / "Gyalog 5 perc" / "Walk 5 min".
pub fn walk(lang: Lang, minutes: u32) -> String {
    match lang {
        Lang::Sk => format!("Pešo {minutes} min"),
        Lang::Hu => format!("Gyalog {minutes} perc"),
        Lang::En => format!("Walk {minutes} min"),
    }
}

/// "1 zastávka" / "3 zastávky" / "7 zastávok"; "3 megálló"; "1 stop" / "3 stops".
pub fn stops(lang: Lang, n: usize) -> String {
    match (lang, n) {
        (Lang::Sk, 1) => "1 zastávka".into(),
        (Lang::Sk, 2..=4) => format!("{n} zastávky"),
        (Lang::Sk, _) => format!("{n} zastávok"),
        (Lang::Hu, _) => format!("{n} megálló"),
        (Lang::En, 1) => "1 stop".into(),
        (Lang::En, _) => format!("{n} stops"),
    }
}

/// "o 9 min" / "9 perc múlva" / "in 9 min".
pub fn in_minutes(lang: Lang, m: i64) -> String {
    match lang {
        Lang::Sk => format!("o {m} min"),
        Lang::Hu => format!("{m} perc múlva"),
        Lang::En => format!("in {m} min"),
    }
}

/// "42 min" / "1 h 05 min".
pub fn duration(lang: Lang, minutes: i64) -> String {
    let unit = if lang == Lang::Hu { "perc" } else { "min" };
    let h = if lang == Lang::Hu { "ó" } else { "h" };
    if minutes < 60 {
        format!("{minutes} {unit}")
    } else {
        format!("{} {h} {:02} {unit}", minutes / 60, minutes % 60)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn slovak_plurals_of_changes() {
        let c = |n| changes(Lang::Sk, n);
        assert_eq!(
            [c(0), c(1), c(3), c(5)],
            ["priamy spoj", "1 prestup", "3 prestupy", "5 prestupov"]
        );
        assert_eq!(duration(Lang::En, 65), "1 h 05 min");
        assert_eq!(duration(Lang::Hu, 42), "42 perc");
        let st = |n| stops(Lang::Sk, n);
        assert_eq!(
            [st(1), st(3), st(7)],
            ["1 zastávka", "3 zastávky", "7 zastávok"]
        );
        assert_eq!(stops(Lang::En, 1), "1 stop");
    }
}
