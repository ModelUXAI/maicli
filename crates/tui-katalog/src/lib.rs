//! Kompilator KDL katalog (F-06): strata `providers` + `auth` + `classes`/`
//! `taxonomy` + `runtime` + cascade flatten, dengan crate `kdl` (KDL v2).
//!
//! Oracle: katalog omp sungguhan (test `#[ignore]`-style via `OMP_CATALOG`
//! — dilewati bila env tidak diset). Semua angka hasil verifikasi di
//! `WORK.md` A-85/A-86/A-87.
#![cfg_attr(
    test,
    allow(
        clippy::unwrap_used,
        clippy::expect_used,
        clippy::panic,
        clippy::indexing_slicing
    )
)]

use std::path::{Path, PathBuf};

use kdl::KdlDocument;

/// Aturan satu provider dari stratum `providers/`.
#[derive(Debug, Clone, PartialEq)]
pub struct AturanProvider {
    /// Id provider (argumen node `provider`).
    pub id: String,
    /// Model default (`default-model`), bila ada.
    pub model_bawaan: Option<String>,
    /// Env var kunci (`env …`); bisa lebih dari satu (urutan file).
    pub env: Vec<String>,
    /// `supports-reasoning-effort #true`.
    pub supports_reasoning_effort: bool,
    /// Kelas yang direferensikan (`class "nama" { … }`) → thinking-mode.
    pub kelas: std::collections::BTreeMap<String, String>,
}

/// Aturan auth satu provider dari stratum `auth/`.
#[derive(Debug, Clone, PartialEq)]
pub struct AturanAuth {
    /// Id auth (argumen node `auth`).
    pub id: String,
    /// Jenis login (`login "api-key"` dsb.).
    pub login: Option<String>,
    /// URL validasi dari `validate url=…`, bila ada.
    pub url_validasi: Option<String>,
}

/// Satu override di `class { override id=… provider=… model=… }`.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct AturanOverride {
    /// Id override.
    pub id: String,
    /// Provider yang disasar.
    pub provider: String,
    /// Model yang disasar.
    pub model: String,
    /// Family yang disasar (bila ada).
    pub family: Option<String>,
}

/// Aturan satu class dari strata `classes/` atau `taxonomy/`.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct AturanClass {
    /// Id class.
    pub id: String,
    /// `namespace "…"`.
    pub namespace: Option<String>,
    /// `bounded "…"` (dapat lebih dari satu).
    pub bounded: Vec<String>,
    /// `family "nama" glob="…"` berurutan.
    pub families: Vec<(String, String)>,
    /// `revision prefix="…"`.
    pub revisi_prefix: Option<String>,
    /// Override yang terdaftar.
    pub overrides: Vec<AturanOverride>,
}

/// Aturan runtime dari stratum `runtime/` (behavior heuristik).
#[derive(Debug, Clone, PartialEq, Default)]
pub struct AturanRuntime {
    /// `openai-responses-heuristic`: prefix model yang masuk.
    pub heuristik_masuk: Vec<String>,
    /// Prefix yang keluar.
    pub heuristik_keluar: Vec<String>,
    /// Substring yang keluar.
    pub heuristik_keluar_substring: Vec<String>,
    /// Provider yang dipensiunkan.
    pub penyedia_dipensiunkan: Vec<String>,
    /// Provider yang diisolasi referensi.
    pub penyedia_isolasi_ref: Vec<String>,
    /// Provider dengan konteks append-only.
    pub penyedia_append_only: Vec<String>,
}

/// Satu aturan cascade hasil flatten selector scope + axis (F-06
/// compile-cascade versi v1): `selector` = cakupan berlaku; `axes` = direktif.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct AturanCascade {
    /// Cakupan: class id.
    pub class: Option<String>,
    /// Cakupan: provider id (bila dari berkas provider).
    pub provider: Option<String>,
    /// Cakupan: api/on-api list.
    pub apis: Vec<String>,
    /// Cakupan: family name.
    pub family: Option<String>,
    /// Cakupan: model ids dari `models …`.
    pub models: Vec<String>,
    /// Direktif axis: nama node → nilai teks (`#true`→"true", angka, string).
    pub axes: std::collections::BTreeMap<String, String>,
}

/// Hasil kompilasi seluruh strata.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Katalog {
    pub providers: Vec<AturanProvider>,
    pub auth: Vec<AturanAuth>,
    /// Strata `classes/` + `taxonomy/` (sama bentuk: `class "id" { … }`).
    pub classes: Vec<AturanClass>,
    /// Stratum `runtime/` (behavior); `None` bila tidak ada.
    pub runtime: Option<AturanRuntime>,
    /// Aturan cascade hasil flatten (providers + classes).
    pub kaskade: Vec<AturanCascade>,
}

impl Katalog {
    /// Jumlah aturan provider.
    pub fn jumlah_provider(&self) -> usize {
        self.providers.len()
    }

    /// Jumlah aturan auth.
    pub fn jumlah_auth(&self) -> usize {
        self.auth.len()
    }

    /// Jumlah aturan class (classes + taxonomy).
    pub fn jumlah_class(&self) -> usize {
        self.classes.len()
    }

    /// Cari aturan provider berdasarkan id.
    pub fn provider(&self, id: &str) -> Option<&AturanProvider> {
        self.providers.iter().find(|p| p.id == id)
    }

    /// Cari aturan class berdasarkan id.
    pub fn class(&self, id: &str) -> Option<&AturanClass> {
        self.classes.iter().find(|c| c.id == id)
    }
}

/// Muat dan susun katalog dari direktori `rules/` omp.
pub fn susun(rules_dir: &Path) -> Result<Katalog, String> {
    let mut katalog = Katalog::default();
    let dir_providers = rules_dir.join("providers");
    let dir_auth = rules_dir.join("auth");
    if !dir_providers.is_dir() {
        return Err(format!(
            "direktori providers tidak ada: {}",
            dir_providers.display()
        ));
    }
    let mut berkas: Vec<PathBuf> = std::fs::read_dir(&dir_providers)
        .map_err(|e| format!("baca {}: {e}", dir_providers.display()))?
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|ext| ext == "kdl"))
        .collect();
    berkas.sort();
    for path in berkas {
        let teks =
            std::fs::read_to_string(&path).map_err(|e| format!("baca {}: {e}", path.display()))?;
        // Berkas stratum lain (on-api: image-tokenization dll.) bukan provider
        // — dilewati, bukan error (F-06: 105 berkas, 98 di antaranya provider).
        if let Some(aturan) =
            susun_provider(&teks).map_err(|e| format!("{}: {e}", path.display()))?
        {
            if let Ok(kaskade) = kaskade_berkas(&teks) {
                katalog.kaskade.extend(kaskade);
            }
            katalog.providers.push(aturan);
        }
    }
    if dir_auth.is_dir() {
        let mut berkas_auth: Vec<PathBuf> = std::fs::read_dir(&dir_auth)
            .map_err(|e| format!("baca {}: {e}", dir_auth.display()))?
            .flatten()
            .map(|e| e.path())
            .filter(|p| p.extension().is_some_and(|ext| ext == "kdl"))
            .collect();
        berkas_auth.sort();
        for path in berkas_auth {
            let teks = std::fs::read_to_string(&path)
                .map_err(|e| format!("baca {}: {e}", path.display()))?;
            if let Some(aturan) =
                susun_auth(&teks).map_err(|e| format!("{}: {e}", path.display()))?
            {
                katalog.auth.push(aturan);
            }
        }
    }
    // Strata classes + taxonomy: bentuk sama (`class "id" { … }`) — dipakai
    // bersama; taxonomy hanya kumpulan class ringkas.
    for strata in ["classes", "taxonomy"] {
        let dir_class = rules_dir.join(strata);
        if !dir_class.is_dir() {
            continue;
        }
        let mut berkas_class: Vec<PathBuf> = std::fs::read_dir(&dir_class)
            .map_err(|e| format!("baca {}: {e}", dir_class.display()))?
            .flatten()
            .map(|e| e.path())
            .filter(|p| p.extension().is_some_and(|ext| ext == "kdl"))
            .collect();
        berkas_class.sort();
        for path in berkas_class {
            let teks = std::fs::read_to_string(&path)
                .map_err(|e| format!("baca {}: {e}", path.display()))?;
            if let Some(aturan) =
                susun_class(&teks).map_err(|e| format!("{}: {e}", path.display()))?
            {
                if let Ok(kaskade) = kaskade_berkas(&teks) {
                    katalog.kaskade.extend(kaskade);
                }
                katalog.classes.push(aturan);
            }
        }
    }
    // Stratum runtime (behavior) — satu berkas.
    let dir_runtime = rules_dir.join("runtime");
    if dir_runtime.is_dir()
        && let Ok(entries) = std::fs::read_dir(&dir_runtime)
    {
        let mut berkas_rt: Vec<PathBuf> = entries
            .flatten()
            .map(|e| e.path())
            .filter(|p| p.extension().is_some_and(|ext| ext == "kdl"))
            .collect();
        berkas_rt.sort();
        for path in berkas_rt {
            let teks = std::fs::read_to_string(&path)
                .map_err(|e| format!("baca {}: {e}", path.display()))?;
            if let Some(aturan) =
                susun_runtime(&teks).map_err(|e| format!("{}: {e}", path.display()))?
            {
                katalog.runtime = Some(aturan);
            }
        }
    }
    Ok(katalog)
}

/// Susun satu berkas provider.
fn susun_provider(teks: &str) -> Result<Option<AturanProvider>, String> {
    let doc: KdlDocument = teks.parse().map_err(|e| format!("parse KDL: {e}"))?;
    let node = doc.nodes().first().ok_or("berkas kosong")?;
    if node.name().value() != "provider" {
        // Berkas stratum lain di direktori yang sama — dilewati, bukan error.
        return Ok(None);
    }
    let id = node
        .entries()
        .iter()
        .find(|e| e.name().is_none())
        .and_then(|e| e.value().as_string())
        .ok_or("provider tanpa argumen id")?
        .to_string();
    let mut aturan = AturanProvider {
        id,
        model_bawaan: None,
        env: Vec::new(),
        supports_reasoning_effort: false,
        kelas: std::collections::BTreeMap::new(),
    };
    if let Some(blok) = node.children() {
        for anak in blok.nodes() {
            match anak.name().value() {
                "default-model" => {
                    aturan.model_bawaan = anak
                        .entries()
                        .iter()
                        .find(|e| e.name().is_none())
                        .and_then(|e| e.value().as_string())
                        .map(str::to_string);
                }
                "env" => {
                    if let Some(nama) = anak
                        .entries()
                        .iter()
                        .find(|e| e.name().is_none())
                        .and_then(|e| e.value().as_string())
                    {
                        aturan.env.push(nama.to_string());
                    }
                }
                "supports-reasoning-effort" => {
                    aturan.supports_reasoning_effort = anak
                        .entries()
                        .first()
                        .and_then(|e| e.value().as_bool())
                        .unwrap_or(false);
                }
                "class" => {
                    let nama_kelas = anak
                        .entries()
                        .iter()
                        .find(|e| e.name().is_none())
                        .and_then(|e| e.value().as_string())
                        .unwrap_or_default()
                        .to_string();
                    let mut mode = String::new();
                    if let Some(blok_kelas) = anak.children() {
                        for dalam in blok_kelas.nodes() {
                            if dalam.name().value() == "thinking-mode" {
                                mode = dalam
                                    .entries()
                                    .first()
                                    .and_then(|e| e.value().as_string())
                                    .unwrap_or_default()
                                    .to_string();
                            }
                        }
                    }
                    aturan.kelas.insert(nama_kelas, mode);
                }
                _ => {}
            }
        }
    }
    Ok(Some(aturan))
}

/// Susun satu berkas auth.
fn susun_auth(teks: &str) -> Result<Option<AturanAuth>, String> {
    let doc: KdlDocument = teks.parse().map_err(|e| format!("parse KDL: {e}"))?;
    let node = doc.nodes().first().ok_or("berkas kosong")?;
    if node.name().value() != "auth" {
        return Ok(None);
    }
    let id = node
        .entries()
        .iter()
        .find(|e| e.name().is_none())
        .and_then(|e| e.value().as_string())
        .ok_or("auth tanpa argumen id")?
        .to_string();
    let mut aturan = AturanAuth {
        id,
        login: None,
        url_validasi: None,
    };
    if let Some(blok) = node.children() {
        for login in blok.nodes() {
            if login.name().value() != "login" {
                continue;
            }
            aturan.login = login
                .entries()
                .iter()
                .find(|e| e.name().is_none())
                .and_then(|e| e.value().as_string())
                .map(str::to_string);
            if let Some(blok_login) = login.children() {
                for dalam in blok_login.nodes() {
                    if dalam.name().value() == "validate"
                        && let Some(url) = dalam.get("url")
                        && let Some(nilai) = url.as_string()
                    {
                        aturan.url_validasi = Some(nilai.to_string());
                    }
                }
            }
        }
    }
    Ok(Some(aturan))
}

/// Susun satu berkas `class` (strata classes atau taxonomy).
fn susun_class(teks: &str) -> Result<Option<AturanClass>, String> {
    let doc: KdlDocument = teks.parse().map_err(|e| format!("parse KDL: {e}"))?;
    let node = doc.nodes().first().ok_or("berkas kosong")?;
    if node.name().value() != "class" {
        return Ok(None); // strata lain di dir sama — dilewati
    }
    let id = node
        .entries()
        .iter()
        .find(|e| e.name().is_none())
        .and_then(|e| e.value().as_string())
        .ok_or("class tanpa argumen id")?
        .to_string();
    let mut aturan = AturanClass {
        id,
        ..AturanClass::default()
    };
    let Some(blok) = node.children() else {
        return Ok(Some(aturan));
    };
    for anak in blok.nodes() {
        match anak.name().value() {
            "namespace" => {
                aturan.namespace = anak
                    .entries()
                    .iter()
                    .find(|e| e.name().is_none())
                    .and_then(|e| e.value().as_string())
                    .map(str::to_string);
            }
            "bounded" => {
                if let Some(nama) = anak
                    .entries()
                    .iter()
                    .find(|e| e.name().is_none())
                    .and_then(|e| e.value().as_string())
                {
                    aturan.bounded.push(nama.to_string());
                }
            }
            "family" => {
                let nama = anak
                    .entries()
                    .iter()
                    .find(|e| e.name().is_none())
                    .and_then(|e| e.value().as_string())
                    .unwrap_or_default()
                    .to_string();
                let glob = anak
                    .get("glob")
                    .and_then(|v| v.as_string())
                    .unwrap_or_default()
                    .to_string();
                aturan.families.push((nama, glob));
            }
            "revision" => {
                aturan.revisi_prefix = anak
                    .get("prefix")
                    .and_then(|v| v.as_string())
                    .map(str::to_string);
            }
            "override" => {
                let ambil = |kunci: &str| -> String {
                    anak.get(kunci)
                        .and_then(|v| v.as_string())
                        .unwrap_or_default()
                        .to_string()
                };
                aturan.overrides.push(AturanOverride {
                    id: ambil("id"),
                    provider: ambil("provider"),
                    model: ambil("model"),
                    family: anak
                        .get("family")
                        .and_then(|v| v.as_string())
                        .map(str::to_string),
                });
            }
            _ => {}
        }
    }
    Ok(Some(aturan))
}

/// Susun stratum `runtime` (node atas `behavior`).
fn susun_runtime(teks: &str) -> Result<Option<AturanRuntime>, String> {
    let doc: KdlDocument = teks.parse().map_err(|e| format!("parse KDL: {e}"))?;
    let Some(behavior) = doc.get("behavior") else {
        return Ok(None);
    };
    let mut aturan = AturanRuntime::default();
    let Some(blok) = behavior.children() else {
        return Ok(Some(aturan));
    };
    for anak in blok.nodes() {
        match anak.name().value() {
            "openai-responses-heuristic" => {
                if let Some(b) = anak.children() {
                    for dalam in b.nodes() {
                        match dalam.name().value() {
                            "include-prefix" | "exclude-prefix" | "exclude-substring" => {
                                for e in dalam.entries() {
                                    if e.name().is_none()
                                        && let Some(s) = e.value().as_string()
                                    {
                                        match dalam.name().value() {
                                            "include-prefix" => {
                                                aturan.heuristik_masuk.push(s.to_string())
                                            }
                                            "exclude-prefix" => {
                                                aturan.heuristik_keluar.push(s.to_string())
                                            }
                                            _ => aturan
                                                .heuristik_keluar_substring
                                                .push(s.to_string()),
                                        }
                                    }
                                }
                            }
                            _ => {}
                        }
                    }
                }
            }
            "retired-providers" => {
                for e in anak.entries() {
                    if e.name().is_none()
                        && let Some(s) = e.value().as_string()
                    {
                        aturan.penyedia_dipensiunkan.push(s.to_string());
                    }
                }
            }
            "reference-isolated-providers" => {
                for e in anak.entries() {
                    if e.name().is_none()
                        && let Some(s) = e.value().as_string()
                    {
                        aturan.penyedia_isolasi_ref.push(s.to_string());
                    }
                }
            }
            "append-only-context-providers" => {
                for e in anak.entries() {
                    if e.name().is_none()
                        && let Some(s) = e.value().as_string()
                    {
                        aturan.penyedia_append_only.push(s.to_string());
                    }
                }
            }
            _ => {}
        }
    }
    Ok(Some(aturan))
}

/// Evaluasi heuristik `openai-responses-heuristic` untuk satu model id:
/// `true` = kemungkinan besar Responses (masuk daftar, tidak keluar).
pub fn heuristik_responses(aturan: &AturanRuntime, model_id: &str) -> bool {
    let id = model_id.to_ascii_lowercase();
    let masuk = aturan
        .heuristik_masuk
        .iter()
        .any(|p| id.starts_with(&p.to_ascii_lowercase()));
    let keluar = aturan
        .heuristik_keluar
        .iter()
        .any(|p| id.starts_with(&p.to_ascii_lowercase()))
        || aturan
            .heuristik_keluar_substring
            .iter()
            .any(|s| id.contains(&s.to_ascii_lowercase()));
    masuk && !keluar
}

/// Apakah provider dipensiunkan (tidak boleh dibangkitkan dari snapshot lama).
pub fn provider_dipensiunkan(aturan: &AturanRuntime, provider: &str) -> bool {
    aturan
        .penyedia_dipensiunkan
        .iter()
        .any(|p| p.eq_ignore_ascii_case(provider))
}

/// Nilai entri pertama node → teks kanonik (bool kecil, angka, string).
fn nilai_teks(node: &kdl::KdlNode) -> String {
    node.entries()
        .iter()
        .find(|e| e.name().is_none())
        .map(|e| {
            let nilai = e.value();
            if let Some(b) = nilai.as_bool() {
                b.to_string()
            } else if let Some(i) = nilai.as_integer() {
                i.to_string()
            } else if let Some(s) = nilai.as_string() {
                s.to_string()
            } else {
                nilai.to_string()
            }
        })
        .unwrap_or_default()
}

/// Flatten satu blok (class/provider) menjadi aturan cascade: node **selain
/// selector** = axis; selector menyempitkan scope untuk axis berikutnya.
fn flatten_blok(
    aturan: &mut Vec<AturanCascade>,
    berjalan: AturanCascade,
    blok: &kdl::KdlDocument,
) -> Result<(), String> {
    let mut scope = berjalan;
    // Axis yang sudah dipakai dalam scope ini (dobel = error keras, omp).
    let mut dipakai: Vec<String> = Vec::new();
    for anak in blok.nodes() {
        let nama = anak.name().value();
        match nama {
            // Selector: mempersempit scope (bukan axis).
            "class" | "provider" | "on" | "on-api" | "on-upstream" | "family" | "revision"
            | "models" | "api" => {
                if nama == "class"
                    && let Some(id) = anak
                        .entries()
                        .iter()
                        .find(|e| e.name().is_none())
                        .and_then(|e| e.value().as_string())
                {
                    scope.class = Some(id.to_string());
                    if let Some(blok_dalam) = anak.children() {
                        flatten_blok(aturan, scope.clone(), blok_dalam)?;
                    }
                    continue;
                }
                match nama {
                    "on-api" | "api" => {
                        for e in anak.entries() {
                            if e.name().is_none()
                                && let Some(s) = e.value().as_string()
                            {
                                scope.apis.push(s.to_string());
                            }
                        }
                    }
                    "family" => {
                        scope.family = anak
                            .entries()
                            .iter()
                            .find(|e| e.name().is_none())
                            .and_then(|e| e.value().as_string())
                            .map(str::to_string);
                    }
                    "models" => {
                        for e in anak.entries() {
                            if e.name().is_none()
                                && let Some(s) = e.value().as_string()
                            {
                                scope.models.push(s.to_string());
                            }
                        }
                        // Axis di dalam blok models mewarisi cakupan models.
                        if let Some(blok_dalam) = anak.children() {
                            flatten_blok(aturan, scope.clone(), blok_dalam)?;
                        }
                    }
                    _ => {} // on/on-upstream/provider/revision: cakupan lain (v1 terbatas)
                }
            }
            // Axis: node biasa → aturan flat.
            _ => {
                if dipakai.iter().any(|n| n == nama) {
                    return Err(format!("axis '{nama}' dobel dalam satu scope"));
                }
                dipakai.push(nama.to_string());
                let mut rule = scope.clone();
                rule.axes.insert(nama.to_string(), nilai_teks(anak));
                aturan.push(rule);
            }
        }
    }
    Ok(())
}

/// Susun aturan cascade dari **satu berkas** provider/class.
pub fn kaskade_berkas(teks: &str) -> Result<Vec<AturanCascade>, String> {
    let doc: KdlDocument = teks.parse().map_err(|e| format!("parse KDL: {e}"))?;
    let mut hasil = Vec::new();
    let Some(node) = doc.nodes().first() else {
        return Ok(hasil);
    };
    let nama_atas = node.name().value();
    if nama_atas != "provider" && nama_atas != "class" {
        return Ok(hasil); // strata lain tidak dikaskadekan
    }
    let mut scope = AturanCascade::default();
    let id = node
        .entries()
        .iter()
        .find(|e| e.name().is_none())
        .and_then(|e| e.value().as_string())
        .map(str::to_string);
    if nama_atas == "provider" {
        scope.provider = id;
    } else {
        scope.class = id;
    }
    if let Some(blok) = node.children() {
        flatten_blok(&mut hasil, scope, blok)?;
    }
    Ok(hasil)
}

/// Cocok model id dengan pola family (glob omp, mis. `*deepseek-flash`).
fn glob_cocok(pola: &str, model_id: &str) -> bool {
    match globset::Glob::new(pola) {
        Ok(glob) => glob.compile_matcher().is_match(model_id),
        Err(_) => pola.eq_ignore_ascii_case(model_id),
    }
}

/// Resolusi cascade: kumpulkan axis yang berlaku untuk (provider, model).
///
/// Urutan penggabungan (makin spesifik menimpa): scope provider → class
/// (families glob cocok model) → aturan dengan `models` eksplisit.
pub fn cari_axis(
    katalog: &Katalog,
    provider: &str,
    model_id: &str,
) -> std::collections::BTreeMap<String, String> {
    let classes_cocok: Vec<&AturanClass> = katalog
        .classes
        .iter()
        .filter(|c| {
            c.families
                .iter()
                .any(|(_, glob)| glob_cocok(glob, model_id))
        })
        .collect();
    let class_ids: Vec<&str> = classes_cocok.iter().map(|c| c.id.as_str()).collect();
    let mut hasil: std::collections::BTreeMap<String, String> = Default::default();
    for r in &katalog.kaskade {
        if let Some(p) = &r.provider
            && !p.eq_ignore_ascii_case(provider)
        {
            continue;
        }
        let class_cocok = match &r.class {
            None => true,
            Some(class) => class_ids.contains(&class.as_str()),
        };
        if !class_cocok {
            continue;
        }
        let models_cocok = r.models.is_empty()
            || r.models
                .iter()
                .any(|m| m.eq_ignore_ascii_case(model_id) || glob_cocok(m, model_id));
        if !models_cocok {
            continue;
        }
        for (kunci, nilai) in &r.axes {
            hasil.insert(kunci.clone(), nilai.clone());
        }
    }
    hasil
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn susun_provider_dari_fixture() {
        let teks = r#"
// komentar harus diabaikan
provider "uji" {
	default-model "uji-v1"
	env "UJI_API_KEY"
	env "UJI_KEY_ALT"
	supports-reasoning-effort #true
	catalog-endpoint-budgets #true
	class "uji" {
		thinking-mode "effort"
	}
}
"#;
        let aturan = susun_provider(teks).expect("susun").expect("provider");
        assert_eq!(aturan.id, "uji");
        assert_eq!(aturan.model_bawaan.as_deref(), Some("uji-v1"));
        assert_eq!(aturan.env, vec!["UJI_API_KEY", "UJI_KEY_ALT"]);
        assert!(aturan.supports_reasoning_effort);
        assert_eq!(aturan.kelas.get("uji").map(String::as_str), Some("effort"));
    }

    #[test]
    fn susun_auth_dari_fixture() {
        let teks = r#"
auth "uji" {
	name "Uji"
	login "api-key" {
		auth-url "https://uji.example/keys"
		validate "models-endpoint" label="uji" url="https://api.uji.example/v1/models"
	}
}
"#;
        let aturan = susun_auth(teks).expect("susun").expect("auth");
        assert_eq!(aturan.id, "uji");
        assert_eq!(aturan.login.as_deref(), Some("api-key"));
        assert_eq!(
            aturan.url_validasi.as_deref(),
            Some("https://api.uji.example/v1/models")
        );
    }

    #[test]
    fn susun_menolak_berkas_kosong_dan_skip_node_asing() {
        assert!(susun_provider("").is_err());
        assert!(susun_provider("provider {\n}").is_err(), "tanpa id");
        // Node atas bukan provider → None (berkas stratum lain di dir sama).
        assert_eq!(susun_provider("auth \"x\" {\n}").expect("ok"), None);
        assert_eq!(susun_auth("provider \"x\" {\n}").expect("ok"), None);
    }

    #[test]
    fn susun_class_dari_fixture() {
        let teks = r#"
class "uji" {
	bound "uji"  // skip: node tak dikenal diterima diam (sesuai audit F-06)
	namespace "uji" bounded=#true
	family "a" glob="*uji-a*"
	revision prefix="uji-v" anywhere=#true
	override id="ov1" provider="uji-prov" model="uji-a" family="a"
}
"#;
        let aturan = susun_class(teks).expect("susun").expect("class");
        assert_eq!(aturan.id, "uji");
        assert_eq!(aturan.namespace.as_deref(), Some("uji"));
        assert_eq!(
            aturan.families,
            vec![("a".to_string(), "*uji-a*".to_string())]
        );
        assert_eq!(aturan.revisi_prefix.as_deref(), Some("uji-v"));
        assert_eq!(aturan.overrides.len(), 1);
        assert_eq!(aturan.overrides[0].provider, "uji-prov");
    }

    #[test]
    fn runtime_fixture_heuristik_dan_pensiun() {
        let teks = r#"
behavior {
	openai-responses-heuristic {
		include-prefix "gpt-" "o1"
		exclude-prefix "tts-"
		exclude-substring "embedding"
	}
	retired-providers "opencode"
	append-only-context-providers "deepseek"
}
"#;
        let rt = susun_runtime(teks).expect("susun").expect("behavior");
        assert_eq!(
            rt.heuristik_masuk,
            vec!["gpt-".to_string(), "o1".to_string()]
        );
        assert_eq!(rt.penyedia_dipensiunkan, vec!["opencode".to_string()]);
        assert_eq!(rt.penyedia_append_only, vec!["deepseek".to_string()]);
        assert!(heuristik_responses(&rt, "gpt-4o"));
        assert!(heuristik_responses(&rt, "o1-mini"));
        assert!(!heuristik_responses(&rt, "tts-1"));
        assert!(!heuristik_responses(&rt, "text-embedding-3"));
        assert!(!heuristik_responses(&rt, "claude-x"));
        assert!(provider_dipensiunkan(&rt, "opencode"));
        assert!(!provider_dipensiunkan(&rt, "deepseek"));
    }

    #[test]
    fn kaskade_fixture_flatten_dan_dobel_axis_ditolak() {
        let teks = r#"
provider "uji" {
	default-model "uji-v1"
	env "UJI_KEY"
	class "uji" {
		thinking-mode "effort"
	}
	supports-reasoning-effort #true
	models "uji-a" "uji-b" {
		clamp-output-to-model-max #true
	}
}
"#;
        let aturan = kaskade_berkas(teks).expect("kaskade");
        assert!(
            aturan
                .iter()
                .any(|r| r.axes.get("default-model").map(String::as_str) == Some("uji-v1"))
        );
        assert!(
            aturan
                .iter()
                .any(|r| r.axes.get("env").map(String::as_str) == Some("UJI_KEY"))
        );
        let thinking = aturan
            .iter()
            .find(|r| r.axes.contains_key("thinking-mode"))
            .expect("thinking");
        assert_eq!(thinking.class.as_deref(), Some("uji"));
        assert_eq!(
            thinking.axes.get("thinking-mode").map(String::as_str),
            Some("effort")
        );
        let clamp = aturan
            .iter()
            .find(|r| r.axes.contains_key("clamp-output-to-model-max"))
            .expect("clamp");
        assert_eq!(clamp.models, vec!["uji-a", "uji-b"]);
        assert_eq!(
            clamp
                .axes
                .get("clamp-output-to-model-max")
                .map(String::as_str),
            Some("true"),
            "bool kecil"
        );
        let dobel = "provider \"x\" {\n\ta #true\n\ta #false\n}";
        assert!(kaskade_berkas(dobel).is_err(), "axis dobel harus ditolak");
    }

    #[test]
    fn cari_axis_fixture_menimpa_makin_spesifik() {
        let katalog = susun_fixture();
        let axes = cari_axis(&katalog, "uji", "uji-a");
        assert_eq!(
            axes.get("default-model").map(String::as_str),
            Some("uji-v1")
        );
        // class menimpa provider.
        assert_eq!(
            axes.get("thinking-mode").map(String::as_str),
            Some("effort")
        );
        // models menimpa semuanya.
        assert_eq!(
            axes.get("clamp-output-to-model-max").map(String::as_str),
            Some("true")
        );
        // Model lain: clamp tidak berlaku.
        let axes_b = cari_axis(&katalog, "uji", "uji-z");
        assert!(!axes_b.contains_key("clamp-output-to-model-max"));
    }

    fn susun_fixture() -> Katalog {
        let teks = r#"
provider "uji" {
	default-model "uji-v1"
	env "UJI_KEY"
	class "uji" {
		thinking-mode "effort"
	}
	supports-reasoning-effort #true
	models "uji-a" "uji-b" {
		clamp-output-to-model-max #true
	}
}
"#;
        let class_teks = r#"
class "uji" {
	family "a" glob="*uji-a*"
}
"#;
        let mut katalog = Katalog::default();
        if let Some(aturan) = susun_provider(teks).expect("provider") {
            katalog.providers.push(aturan);
        }
        if let Some(aturan) = susun_class(class_teks).expect("class") {
            katalog.classes.push(aturan);
        }
        katalog.kaskade = kaskade_berkas(teks).expect("kaskade");
        katalog
    }

    #[test]
    fn katalog_omp_nyata_terbaca_penuh() {
        // Oracle: katalog omp yang ada di mesin (checkout hanya-baca).
        let Some(root) = std::env::var_os("OMP_CATALOG") else {
            eprintln!("OMP_CATALOG tidak diset — katalog nyata dilewati");
            return;
        };
        let rules = Path::new(&root).join("rules");
        if !rules.is_dir() {
            eprintln!("{} bukan rules — dilewati", rules.display());
            return;
        }
        let katalog = susun(&rules).expect("susun katalog omp");
        // 105 berkas providers (98 berisi aturan provider) + 109 auth (108 auth)
        // + 19 classes + 23 taxonomy; 2 berkas taxonomy khusus
        // (collapse/discovery) bukan class → 40 aturan class.
        assert_eq!(katalog.jumlah_provider(), 98, "provider");
        assert_eq!(katalog.jumlah_auth(), 108, "auth");
        assert_eq!(katalog.jumlah_class(), 40, "class (19+21)");
        // Id class muncul di dua strata (classes + taxonomy) — gabungkan.
        let ds_semua: Vec<&AturanClass> = katalog
            .classes
            .iter()
            .filter(|c| c.id == "deepseek")
            .collect();
        assert_eq!(ds_semua.len(), 2, "classes + taxonomy keduanya ada");
        assert_eq!(
            ds_semua.iter().map(|c| c.families.len()).sum::<usize>(),
            15,
            "family gabungan"
        );
        assert_eq!(
            ds_semua.iter().map(|c| c.overrides.len()).sum::<usize>(),
            7,
            "7 override (gabungan)"
        );
        assert!(
            ds_semua
                .iter()
                .any(|c| c.namespace.as_deref() == Some("deepseek")),
            "namespace deepseek"
        );
        assert!(
            ds_semua.iter().any(|c| c
                .families
                .iter()
                .any(|(nama, glob)| nama == "flash" && glob == "*deepseek-flash")),
            "family flash v4.1"
        );
        let ds = katalog.provider("deepseek").expect("deepseek ada");
        assert_eq!(ds.env, vec!["DEEPSEEK_API_KEY"]);
        assert_eq!(ds.model_bawaan.as_deref(), Some("deepseek-v4-pro"));
        assert!(ds.supports_reasoning_effort);
        // Runtime oracle.
        let rt = katalog.runtime.as_ref().expect("runtime ada");
        assert!(rt.penyedia_dipensiunkan.contains(&"opencode".to_string()));
        assert!(rt.penyedia_append_only.contains(&"deepseek".to_string()));
        assert!(rt.heuristik_masuk.contains(&"gpt-".to_string()));
        assert!(heuristik_responses(rt, "gpt-5.5"));
        assert!(!heuristik_responses(rt, "text-embedding-3-large"));
        assert!(!heuristik_responses(rt, "glm-5.3-flash"));
    }

    #[test]
    fn runtime_omp_oracle_terpisah() {
        let Some(root) = std::env::var_os("OMP_CATALOG") else {
            return;
        };
        let rules = std::path::Path::new(&root).join("rules");
        if !rules.is_dir() {
            return;
        }
        let katalog = susun(&rules).expect("susun");
        let rt = katalog.runtime.as_ref().expect("runtime ada");
        assert!(heuristik_responses(rt, "gpt-5.5"));
        assert!(!heuristik_responses(rt, "glm-5.3-flash"));
    }

    #[test]
    fn kaskade_omp_oracle_jumlah_aturan() {
        let Some(root) = std::env::var_os("OMP_CATALOG") else {
            return;
        };
        let dir = std::path::Path::new(&root).join("rules").join("providers");
        if !dir.is_dir() {
            return;
        }
        let mut total = 0usize;
        for path in std::fs::read_dir(&dir).expect("dir").flatten() {
            let path = path.path();
            if path.extension().is_some_and(|ext| ext == "kdl")
                && let Ok(teks) = std::fs::read_to_string(&path)
            {
                total += kaskade_berkas(&teks).expect("kaskade per berkas").len();
            }
        }
        assert!(total > 300, "aturan cascade = {total}");
    }

    #[test]
    fn cari_axis_omp_oracle_deepseek() {
        let Some(root) = std::env::var_os("OMP_CATALOG") else {
            return;
        };
        let rules = std::path::Path::new(&root).join("rules");
        if !rules.is_dir() {
            return;
        }
        let katalog = susun(&rules).expect("susun");
        // Model flash v4.1: axis class effort + supports-reasoning-effort.
        let axes = cari_axis(&katalog, "deepseek", "deepseek-v4.1-flash");
        assert_eq!(
            axes.get("supports-reasoning-effort").map(String::as_str),
            Some("true")
        );
        assert_eq!(
            axes.get("thinking-mode").map(String::as_str),
            Some("effort")
        );
        assert_eq!(
            axes.get("default-model").map(String::as_str),
            Some("deepseek-v4-pro")
        );
    }
}
