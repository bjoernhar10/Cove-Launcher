/**
 * ~40 fictional games. All names, studios and texts are invented.
 * Artwork URLs are null → the UI renders calm CSS placeholder covers.
 */
import type { Game, Genre, Platform } from "@cove/shared";
import { createRandom } from "./random.ts";

interface Seed {
  title: string;
  genres: Genre[];
  year: number;
  summary: string;
}

const SEEDS: Seed[] = [
  { title: "Tidewater", genres: ["adventure", "indie"], year: 2024, summary: "Ein Fischerdorf, eine steigende Flut und ein Leuchtturm, der Geheimnisse bewahrt." },
  { title: "Hollow Lantern", genres: ["horror", "adventure"], year: 2023, summary: "Erkunde ein verlassenes Sanatorium nur mit einer flackernden Laterne." },
  { title: "Paper Orbit", genres: ["puzzle", "indie"], year: 2025, summary: "Falte Planeten aus Papier und bringe sie in eine stabile Umlaufbahn." },
  { title: "Ashen Meridian", genres: ["rpg", "action"], year: 2022, summary: "Ein Rollenspiel über eine Welt, in der die Sonne seit hundert Jahren nicht aufging." },
  { title: "Quiet Harbor", genres: ["simulation", "indie"], year: 2021, summary: "Führe einen kleinen Hafen durch vier ruhige Jahreszeiten." },
  { title: "Glass Cartographer", genres: ["adventure", "puzzle"], year: 2026, summary: "Kartiere eine Stadt, deren Straßen sich jede Nacht neu anordnen." },
  { title: "Ironbark", genres: ["strategy"], year: 2020, summary: "Baue eine Waldsiedlung auf, ohne den Wald zu zerstören." },
  { title: "Low Tide Society", genres: ["rpg", "indie"], year: 2024, summary: "Ein Gesellschaftsdrama unter Strandgutsammlern an der Nordsee." },
  { title: "Saltmarsh", genres: ["simulation"], year: 2019, summary: "Bewirtschafte eine Salzwiese und schütze sie vor den Gezeiten." },
  { title: "The Long Commute", genres: ["adventure", "indie"], year: 2023, summary: "Eine Geschichte, erzählt in fünfzig Zugfahrten durch dieselbe Stadt." },
  { title: "Ember & Oak", genres: ["rpg", "adventure"], year: 2025, summary: "Zwei Geschwister, eine Schmiede und ein Königreich am Abgrund." },
  { title: "Northbound", genres: ["action", "adventure"], year: 2021, summary: "Überquere eine gefrorene Tundra mit einem Hundeschlitten und wenig Proviant." },
  { title: "Velvet Static", genres: ["shooter", "action"], year: 2022, summary: "Ein taktischer Shooter in den Funkstudios einer stillgelegten Sendeanstalt." },
  { title: "Copper Valley Racing", genres: ["racing", "sports"], year: 2024, summary: "Rallyes auf Schotterpisten durch ein altes Bergbaugebiet." },
  { title: "Lighthouse Keeper", genres: ["simulation", "indie"], year: 2018, summary: "Halte das Licht am Brennen – Nacht für Nacht, Sturm für Sturm." },
  { title: "Moss Protocol", genres: ["strategy", "simulation"], year: 2026, summary: "Begrüne eine verlassene Raumstation, Modul für Modul." },
  { title: "Stonefield Tactics", genres: ["strategy"], year: 2023, summary: "Rundenbasierte Gefechte auf handgezeichneten Landkarten." },
  { title: "Driftwood", genres: ["adventure", "puzzle"], year: 2020, summary: "Baue aus Treibholz ein Floß und folge dem Fluss bis zum Meer." },
  { title: "Hearthlight", genres: ["rpg", "simulation"], year: 2025, summary: "Führe ein Gasthaus an einer Handelsstraße und höre Geschichten zu." },
  { title: "Signal Lost", genres: ["horror", "puzzle"], year: 2024, summary: "Entschlüssele Funksprüche einer verschollenen Expedition." },
  { title: "Small Hours", genres: ["adventure", "indie"], year: 2022, summary: "Eine Nacht in einem 24-Stunden-Diner und die Menschen, die dort landen." },
  { title: "Thousand Lakes", genres: ["simulation", "sports"], year: 2021, summary: "Angeln, Paddeln und Zelten in einer weiten Seenlandschaft." },
  { title: "Rust Belt Derby", genres: ["racing", "action"], year: 2019, summary: "Stockcar-Rennen auf verlassenen Fabrikgeländen." },
  { title: "Ninth Floor", genres: ["puzzle", "horror"], year: 2025, summary: "Ein Bürogebäude mit acht Stockwerken – und einem neunten, das es nicht geben dürfte." },
  { title: "Pale Garden", genres: ["platformer", "indie"], year: 2023, summary: "Ein stiller Plattformer über einen Garten, der langsam seine Farbe verliert." },
  { title: "Orchard Kingdom", genres: ["strategy", "simulation"], year: 2024, summary: "Regiere ein Königreich, dessen Wohlstand an seinen Apfelbäumen hängt." },
  { title: "Fieldnotes", genres: ["adventure", "indie"], year: 2026, summary: "Als Botanikerin dokumentierst du eine abgelegene Insel." },
  { title: "Bramble Run", genres: ["platformer", "action"], year: 2022, summary: "Ein flinker Fuchs, ein Dornengestrüpp und sehr knappe Sprünge." },
  { title: "Cinder Coast", genres: ["action", "rpg"], year: 2026, summary: "Ein Action-Rollenspiel an einer vulkanischen Küste." },
  { title: "Wireframe Heart", genres: ["puzzle", "indie"], year: 2021, summary: "Setze ein Herz aus Drahtgittern wieder zusammen – Linie für Linie." },
  { title: "Kite Season", genres: ["sports", "indie"], year: 2020, summary: "Drachensteigen als Wettkampf, mit Wind, Thermik und viel Geduld." },
  { title: "Granite & Gale", genres: ["strategy", "action"], year: 2025, summary: "Verteidige eine Bergfestung gegen Stürme und Belagerer." },
  { title: "Midnight Ferry", genres: ["adventure", "horror"], year: 2023, summary: "Die letzte Fähre des Tages fährt nie dort an, wo sie soll." },
  { title: "Tin Soldiers", genres: ["strategy"], year: 2018, summary: "Taktische Schlachten im Kinderzimmer mit aufziehbaren Blechfiguren." },
  { title: "Waypoint Zero", genres: ["shooter", "action"], year: 2026, summary: "Ein kooperativer Shooter in einer Forschungsbasis im ewigen Eis." },
  { title: "Foxglove", genres: ["rpg", "indie"], year: 2024, summary: "Ein Kräuterkunde-Rollenspiel in einem Dorf voller Gerüchte." },
  { title: "Undertow", genres: ["action", "adventure"], year: 2022, summary: "Tauche zu einem gesunkenen Frachter und entkomme der Strömung." },
  { title: "Clockwork Pastures", genres: ["simulation", "puzzle"], year: 2025, summary: "Ein Bauernhof, auf dem alles von Zahnrädern angetrieben wird." },
  { title: "Starling", genres: ["platformer", "adventure"], year: 2021, summary: "Fliege als kleiner Vogel durch einen Schwarm von tausenden." },
  { title: "Archive of Echoes", genres: ["puzzle", "adventure"], year: 2026, summary: "Ordne die Erinnerungen einer verstorbenen Archivarin." },
];

const STUDIOS = [
  "Little Fern Studio",
  "Harbor & Pine",
  "Slow Signal",
  "Kestrel Games",
  "Basalt Interactive",
  "Moth & Lamp",
  "Glasswing",
  "Ninefold",
  "Paper Boat Collective",
  "Grey Heron Works",
  "Ostwind Spiele",
  "Quarry Lane",
];

const PUBLISHERS = ["Cove Sample Publishing", "Northfield Media", "Small Press Games", "Self-published"];

const DESCRIPTION_TAIL = [
  "Jede Entscheidung hinterlässt Spuren – in der Welt und bei den Menschen, die dir begegnen.",
  "Mit handgezeichneten Schauplätzen, einem ruhigen Soundtrack und ohne Zeitdruck.",
  "Unterstützt Controller, frei belegbare Tasten und eine Vorlesefunktion für alle Texte.",
  "Eine Spielzeit von etwa zehn bis fünfzehn Stunden, mit optionalen Aufgaben für Neugierige.",
];

export function slugify(title: string): string {
  return title
    .toLowerCase()
    .replace(/&/g, "and")
    .replace(/[^a-z0-9]+/g, "-")
    .replace(/(^-|-$)/g, "");
}

function pad(n: number): string {
  return String(n).padStart(2, "0");
}

export function createGames(): Game[] {
  const random = createRandom(7);
  return SEEDS.map((seed, index): Game => {
    const platforms: Platform[] = ["windows"];
    if (random() < 0.65) platforms.push("linux");
    if (random() < 0.3) platforms.push("macos");
    const month = 1 + Math.floor(random() * 12);
    const day = 1 + Math.floor(random() * 28);
    const tail = DESCRIPTION_TAIL[index % DESCRIPTION_TAIL.length] ?? "";
    const screenshotCount = 4 + Math.floor(random() * 3);
    const capped = seed.year === 2026 ? Math.min(month, 10) : month;
    return {
      id: slugify(seed.title),
      title: seed.title,
      summary: seed.summary,
      description: `${seed.summary}\n\n${tail}\n\nDieses Spiel ist ein Platzhalter mit erfundenen Daten. Echte Inhalte liefern später die Store-Connectors.`,
      genres: seed.genres,
      platforms,
      releaseDate: `${String(seed.year)}-${pad(capped)}-${pad(day)}`,
      developer: STUDIOS[index % STUDIOS.length] ?? "Unbekannt",
      publisher: PUBLISHERS[index % PUBLISHERS.length] ?? "Unbekannt",
      rating: index % 9 === 8 ? null : 58 + Math.floor(random() * 38),
      popularity: Math.floor(random() * 100),
      artwork: {
        cover: { url: null, alt: `Cover von ${seed.title}` },
        header: { url: null, alt: `Titelbild von ${seed.title}` },
        screenshots: Array.from({ length: screenshotCount }, (_, i) => ({
          url: null,
          alt: `Screenshot ${String(i + 1)} von ${seed.title}`,
        })),
      },
    };
  });
}
