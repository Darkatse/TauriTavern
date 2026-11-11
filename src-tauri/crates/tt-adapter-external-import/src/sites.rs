//! Every origin a recognized site's content comes from must be in tt-application's
//! `PRE_APPROVED_IMPORT_SOURCES`: importing from these sites never asks.

use std::sync::Arc;

use async_trait::async_trait;
use percent_encoding::percent_decode_str;
use serde::Deserialize;
use serde::de::DeserializeOwned;
use serde_json::value::RawValue;
use tt_domain::errors::DomainError;
use tt_ports::external_import::{
    ContentKind, DownloadByteLimit, DownloadSources, ExternalImportDownloader, ImportContent,
    ImportContentResolver, parse_external_import_url,
};
use url::Url;
use uuid::Uuid;

const AICC_HOST: &str = "aicharactercards.com";
const AICC_CARD_API: &str = "https://aicharactercards.com/wp-json/pngapi/v1/image";
const BOTBOORU_HOST: &str = "botbooru.com";
const BOTBOORU_CARD_API: &str = "https://botbooru.com/download/png";
const CHUB_HOSTS: &[&str] = &[
    "chub.ai",
    "www.chub.ai",
    "characterhub.org",
    "www.characterhub.org",
];
const CHUB_API: &str = "https://api.chub.ai/api";
const CHUB_METADATA_LIMIT: DownloadByteLimit = DownloadByteLimit {
    label: "Chub metadata",
    max_bytes: 1024 * 1024,
};
const PYGMALION_HOSTS: &[&str] = &["pygmalion.chat", "www.pygmalion.chat"];
const PYGMALION_EXPORT_API: &str = "https://server.pygmalion.chat/api/export/character";
const PYGMALION_EXPORT_LIMIT: DownloadByteLimit = DownloadByteLimit {
    label: "Pygmalion export",
    max_bytes: 16 * 1024 * 1024,
};
const RISU_HOST: &str = "realm.risuai.net";
const RISU_CARD_API: &str = "https://realm.risuai.net/api/v1/download/png-v3";

pub struct SiteImportContentResolver {
    downloader: Arc<dyn ExternalImportDownloader>,
}

impl SiteImportContentResolver {
    pub fn new(downloader: Arc<dyn ExternalImportDownloader>) -> Self {
        Self { downloader }
    }

    async fn fetch_json<T: DeserializeOwned>(
        &self,
        url: Url,
        limit: DownloadByteLimit,
        sources: &dyn DownloadSources,
    ) -> Result<T, DomainError> {
        let bytes = self
            .downloader
            .fetch_bytes(url.clone(), limit, sources)
            .await?;
        serde_json::from_slice(&bytes).map_err(|error| unexpected_response(&url, error))
    }
}

#[async_trait]
impl ImportContentResolver for SiteImportContentResolver {
    async fn resolve(
        &self,
        url: &str,
        sources: &dyn DownloadSources,
    ) -> Result<ImportContent, DomainError> {
        Ok(match ContentReference::parse(url)? {
            ContentReference::Direct(url) => ImportContent::File {
                url,
                kind: ContentKind::Character,
                name: None,
            },
            ContentReference::ChubCharacter(project) => {
                let url = project.metadata_url("characters");
                let metadata: ChubMetadata<ChubCharacterNode> = self
                    .fetch_json(url.clone(), CHUB_METADATA_LIMIT, sources)
                    .await?;
                ImportContent::File {
                    url: linked_url(&url, &metadata.node.max_res_url)?,
                    kind: ContentKind::Character,
                    name: Some(project.project),
                }
            }
            ContentReference::ChubLorebook(project) => {
                let metadata: ChubMetadata<ChubLorebookNode> = self
                    .fetch_json(
                        project.metadata_url("lorebooks"),
                        CHUB_METADATA_LIMIT,
                        sources,
                    )
                    .await?;
                let url = format!(
                    "{CHUB_API}/v4/projects/{}/repository/files/raw%252Fsillytavern_raw.json/raw",
                    metadata.node.id
                );
                ImportContent::File {
                    url: Url::parse(&url).expect("Chub lorebook file URL is valid"),
                    kind: ContentKind::Lorebook,
                    name: Some(project.project),
                }
            }
            ContentReference::Pygmalion(id) => {
                let url = site_url(PYGMALION_EXPORT_API, &[&id, "v2"]);
                let export: PygmalionExport = self
                    .fetch_json(url.clone(), PYGMALION_EXPORT_LIMIT, sources)
                    .await?;
                let card: PygmalionCard = serde_json::from_str(export.character.get())
                    .map_err(|error| unexpected_response(&url, error))?;
                ImportContent::Card {
                    card_json: export.character.get().to_string(),
                    image: linked_url(&url, &card.data.avatar)?,
                    name: id,
                }
            }
        })
    }
}

enum ContentReference {
    Direct(Url),
    ChubCharacter(ChubProject),
    ChubLorebook(ChubProject),
    Pygmalion(String),
}

impl ContentReference {
    fn parse(raw: &str) -> Result<Self, DomainError> {
        let raw = raw.trim();
        if let Err(url::ParseError::RelativeUrlWithoutBase) = Url::parse(raw) {
            let segments = raw
                .split('/')
                .filter(|segment| !segment.is_empty())
                .map(str::to_string)
                .collect::<Vec<_>>();
            let recognized = match segments.as_slice() {
                [id] => pygmalion_id(id).map(Self::Pygmalion),
                [namespace, user, id]
                    if namespace.eq_ignore_ascii_case("AICC")
                        && is_number(user)
                        && is_number(id) =>
                {
                    Some(Self::Direct(site_url(AICC_CARD_API, &[user, id])))
                }
                segments => chub_reference(segments),
            };
            return recognized.ok_or_else(|| {
                DomainError::InvalidData(format!("Not a URL or a recognized ID: {raw}"))
            });
        }

        let url = parse_external_import_url(raw)?;
        let segments = url
            .path_segments()
            .into_iter()
            .flatten()
            .filter(|segment| !segment.is_empty())
            .map(|segment| percent_decode_str(segment).decode_utf8_lossy().into_owned())
            .collect::<Vec<_>>();
        let recognized = match url.host_str() {
            Some(host) if CHUB_HOSTS.contains(&host) => chub_reference(&segments),
            Some(host) if PYGMALION_HOSTS.contains(&host) => page_id(&segments, "character")
                .and_then(pygmalion_id)
                .map(Self::Pygmalion),
            Some(RISU_HOST) => page_id(&segments, "character")
                .filter(|id| is_risu_id(id))
                .map(|id| Self::Direct(risu_card_url(id))),
            Some(AICC_HOST) => page_id(&segments, "cards")
                .filter(|id| is_number(id))
                .map(|id| Self::Direct(site_url(AICC_CARD_API, &["cards", id]))),
            Some(BOTBOORU_HOST) => page_id(&segments, "character")
                .filter(|id| is_number(id))
                .map(|id| Self::Direct(site_url(BOTBOORU_CARD_API, &[id]))),
            _ => None,
        };
        Ok(recognized.unwrap_or(Self::Direct(url)))
    }
}

fn chub_reference(segments: &[String]) -> Option<ContentReference> {
    let is = |segment: &str, namespace: &str| segment.eq_ignore_ascii_case(namespace);
    let project = |creator: &str, project: &str| ChubProject {
        creator: creator.to_string(),
        project: project.to_string(),
    };
    match segments {
        [namespace, creator, name, ..] if is(namespace, "characters") => {
            Some(ContentReference::ChubCharacter(project(creator, name)))
        }
        [namespace, creator, name, ..] if is(namespace, "lorebooks") => {
            Some(ContentReference::ChubLorebook(project(creator, name)))
        }
        [creator, name] if !is(creator, "characters") && !is(creator, "lorebooks") => {
            Some(ContentReference::ChubCharacter(project(creator, name)))
        }
        _ => None,
    }
}

fn page_id<'a>(segments: &'a [String], page: &str) -> Option<&'a str> {
    match segments {
        [name, id] if name.eq_ignore_ascii_case(page) => Some(id),
        _ => None,
    }
}

fn is_number(id: &str) -> bool {
    !id.is_empty() && id.bytes().all(|byte| byte.is_ascii_digit())
}

/// A hyphenated UUID, lowercased as Pygmalion writes it.
fn pygmalion_id(id: &str) -> Option<String> {
    let uuid = Uuid::try_parse(id).ok().filter(|_| id.len() == 36)?;
    Some(uuid.hyphenated().to_string())
}

fn is_risu_id(id: &str) -> bool {
    !id.is_empty()
        && id
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit() || byte == b'-')
}

fn risu_card_url(id: &str) -> Url {
    let mut url = site_url(RISU_CARD_API, &[id]);
    url.set_query(Some("non_commercial=true"));
    url
}

fn site_url(base: &str, segments: &[&str]) -> Url {
    let mut url = Url::parse(base).expect("site API URLs are valid");
    url.path_segments_mut()
        .expect("site API URLs have paths")
        .extend(segments);
    url
}

fn unexpected_response(url: &Url, error: impl std::fmt::Display) -> DomainError {
    DomainError::InternalError(format!("Unexpected response from {url}: {error}"))
}

fn linked_url(response: &Url, link: &str) -> Result<Url, DomainError> {
    parse_external_import_url(link)
        .map_err(|_| unexpected_response(response, format!("not a download URL: {link:?}")))
}

struct ChubProject {
    creator: String,
    project: String,
}

impl ChubProject {
    fn metadata_url(&self, namespace: &str) -> Url {
        site_url(CHUB_API, &[namespace, &self.creator, &self.project])
    }
}

#[derive(Deserialize)]
struct ChubMetadata<T> {
    node: T,
}

#[derive(Deserialize)]
struct ChubCharacterNode {
    max_res_url: String,
}

#[derive(Deserialize)]
struct ChubLorebookNode {
    id: u64,
}

#[derive(Deserialize)]
struct PygmalionExport {
    character: Box<RawValue>,
}

#[derive(Deserialize)]
struct PygmalionCard {
    data: PygmalionCardData,
}

#[derive(Deserialize)]
struct PygmalionCardData {
    avatar: String,
}

#[cfg(test)]
mod tests {
    use std::path::Path;
    use std::sync::Mutex;

    use tt_ports::external_import::DownloadedFile;

    use super::*;
    use ContentKind::{Character, Lorebook};

    const CHUB_CARD: &str = "https://avatars.charhub.io/avatars/a/furina/chara_card_v2.png";
    const CHUB_LOREBOOK: &str =
        "https://api.chub.ai/api/v4/projects/782/repository/files/raw%252Fsillytavern_raw.json/raw";
    const PYGMALION_ID: &str = "a7ca95a1-0c88-4e23-91b3-149db1e78ab9";
    const PYGMALION_AVATAR: &str = "https://assets.pygmalion.chat/62f1c02c";
    const PYGMALION_CARD: &str = r#"{"spec":"chara_card_v2","data":{"name":"Bronya","avatar":"https://assets.pygmalion.chat/62f1c02c"}}"#;

    async fn resolve(input: &str) -> (Vec<String>, Result<ImportContent, DomainError>) {
        let apis = Arc::new(SiteApis::default());
        let content = SiteImportContentResolver::new(apis.clone())
            .resolve(input, &AllowAll)
            .await;
        let requests = apis.requests.lock().unwrap().clone();
        (requests, content)
    }

    fn file(url: &str, kind: ContentKind, name: Option<&str>) -> ImportContent {
        ImportContent::File {
            url: Url::parse(url).unwrap(),
            kind,
            name: name.map(str::to_string),
        }
    }

    #[tokio::test]
    async fn links_and_ids_resolve_to_what_their_site_serves() {
        let chub_character = ["https://api.chub.ai/api/characters/a/furina"];
        let chub_lorebook = ["https://api.chub.ai/api/lorebooks/a/world"];
        let risu_id = "3ca54c71-6efe-46a2-b9d0-4f62df23d712";
        let aicc_card =
            |path: &str| format!("https://aicharactercards.com/wp-json/pngapi/v1/image/{path}");
        let risu_card = format!(
            "https://realm.risuai.net/api/v1/download/png-v3/{risu_id}?non_commercial=true"
        );
        let pygmalion_export = [format!(
            "https://server.pygmalion.chat/api/export/character/{PYGMALION_ID}/v2"
        )];
        let pygmalion_card = || ImportContent::Card {
            card_json: PYGMALION_CARD.to_string(),
            image: Url::parse(PYGMALION_AVATAR).unwrap(),
            name: PYGMALION_ID.to_string(),
        };
        let none: &[&str] = &[];
        for (input, requests, expected) in [
            (
                "https://chub.ai/characters/a/furina".to_string(),
                &chub_character[..],
                file(CHUB_CARD, Character, Some("furina")),
            ),
            (
                "https://www.characterhub.org/a/furina/?tab=x#top".to_string(),
                &chub_character[..],
                file(CHUB_CARD, Character, Some("furina")),
            ),
            (
                " a/furina ".to_string(),
                &chub_character[..],
                file(CHUB_CARD, Character, Some("furina")),
            ),
            (
                "https://chub.ai/Lorebooks/a/world".to_string(),
                &chub_lorebook[..],
                file(CHUB_LOREBOOK, Lorebook, Some("world")),
            ),
            (
                "lorebooks/a/world".to_string(),
                &chub_lorebook[..],
                file(CHUB_LOREBOOK, Lorebook, Some("world")),
            ),
            (
                format!("https://realm.risuai.net/character/{risu_id}/"),
                none,
                file(&risu_card, Character, None),
            ),
            (
                "https://aicharactercards.com/cards/204".to_string(),
                none,
                file(&aicc_card("cards/204"), Character, None),
            ),
            (
                "AICC/645/204".to_string(),
                none,
                file(&aicc_card("645/204"), Character, None),
            ),
            (
                "https://botbooru.com/character/123".to_string(),
                none,
                file("https://botbooru.com/download/png/123", Character, None),
            ),
            (
                "https://chub.ai.example/characters/a/furina".to_string(),
                none,
                file(
                    "https://chub.ai.example/characters/a/furina",
                    Character,
                    None,
                ),
            ),
            (
                "https://chub.ai/lorebooks/world".to_string(),
                none,
                file("https://chub.ai/lorebooks/world", Character, None),
            ),
            (
                format!(
                    "https://pygmalion.chat/character/{}",
                    PYGMALION_ID.to_uppercase()
                ),
                &[pygmalion_export[0].as_str()][..],
                pygmalion_card(),
            ),
            (
                PYGMALION_ID.to_string(),
                &[pygmalion_export[0].as_str()][..],
                pygmalion_card(),
            ),
        ] {
            let (requested, content) = resolve(&input).await;
            assert_eq!(requested, requests, "{input}");
            assert_eq!(content.expect("resolve"), expected, "{input}");
        }

        let (requested, unrecognized) = resolve("Loreena~cb3a1b53.gz").await;
        assert!(requested.is_empty());
        assert!(matches!(unrecognized, Err(DomainError::InvalidData(_))));
    }

    struct AllowAll;

    #[async_trait]
    impl DownloadSources for AllowAll {
        async fn allow(&self, _url: &Url) -> Result<(), DomainError> {
            Ok(())
        }
    }

    /// Answers each site's JSON API.
    #[derive(Default)]
    struct SiteApis {
        requests: Mutex<Vec<String>>,
    }

    #[async_trait]
    impl ExternalImportDownloader for SiteApis {
        async fn fetch_bytes(
            &self,
            url: Url,
            _limit: DownloadByteLimit,
            _sources: &dyn DownloadSources,
        ) -> Result<Vec<u8>, DomainError> {
            self.requests.lock().unwrap().push(url.to_string());
            let body = match url.host_str() {
                Some("api.chub.ai") => {
                    format!(r#"{{"node":{{"id":782,"max_res_url":"{CHUB_CARD}"}}}}"#)
                }
                Some("server.pygmalion.chat") => format!(r#"{{"character":{PYGMALION_CARD}}}"#),
                host => unimplemented!("no JSON from {host:?}"),
            };
            Ok(body.into_bytes())
        }

        async fn fetch_to_file(
            &self,
            _url: Url,
            _path: &Path,
            _sources: &dyn DownloadSources,
        ) -> Result<DownloadedFile, DomainError> {
            unimplemented!("resolving reads only site JSON")
        }
    }
}
