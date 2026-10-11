use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Kind {
    Songs,
    Albums,
    Artists,
    Playlists,
    LibrarySongs,
    LibraryAlbums,
    LibraryPlaylists,
    #[serde(other)]
    Other,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(from = "Resource")]
pub struct Item {
    pub id: String,
    pub kind: Kind,
    pub name: Option<String>,
    pub artist: Option<String>,
    pub album: Option<String>,
    pub duration_ms: Option<u64>,
    pub artwork: Option<String>,
    pub track_count: Option<u32>,
}

#[derive(Deserialize)]
struct Resource {
    id: String,
    #[serde(rename = "type")]
    kind: Kind,
    #[serde(default)]
    attributes: Attributes,
}

#[derive(Default, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Attributes {
    name: Option<String>,
    artist_name: Option<String>,
    album_name: Option<String>,
    duration_in_millis: Option<u64>,
    artwork: Option<Artwork>,
    track_count: Option<u32>,
}

#[derive(Deserialize)]
struct Artwork {
    url: Option<String>,
}

impl From<Resource> for Item {
    fn from(r: Resource) -> Self {
        let a = r.attributes;
        Item {
            id: r.id,
            kind: r.kind,
            name: a.name,
            artist: a.artist_name,
            album: a.album_name,
            duration_ms: a.duration_in_millis,
            artwork: a.artwork.and_then(|art| art.url),
            track_count: a.track_count,
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Page {
    #[serde(rename(deserialize = "data"), default)]
    pub items: Vec<Item>,
    pub next: Option<String>,
    #[serde(rename(deserialize = "meta"), default, deserialize_with = "total")]
    pub total: Option<u32>,
}

fn total<'de, D: serde::Deserializer<'de>>(d: D) -> Result<Option<u32>, D::Error> {
    #[derive(Deserialize)]
    struct Meta {
        total: Option<u32>,
    }
    Ok(Option::<Meta>::deserialize(d)?.and_then(|m| m.total))
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct SearchResults {
    pub songs: Page,
    pub albums: Page,
    pub artists: Page,
    pub playlists: Page,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct Detail {
    pub item: Item,
    pub tracks: Page,
}

#[derive(Deserialize)]
pub(crate) struct Envelope<T> {
    pub data: Vec<T>,
}

#[derive(Deserialize)]
pub(crate) struct Searched {
    #[serde(default)]
    pub results: SearchResults,
}

#[derive(Deserialize)]
pub(crate) struct WithTracks {
    #[serde(flatten)]
    pub item: Item,
    #[serde(default)]
    pub relationships: Relationships,
}

#[derive(Default, Deserialize)]
pub(crate) struct Relationships {
    #[serde(default)]
    pub tracks: Page,
}

pub fn art_url(template: &str, px: u32) -> Option<String> {
    let px = px.to_string();
    let url = template.replace("{w}", &px).replace("{h}", &px);
    (!url.contains(['{', '}'])).then_some(url)
}
