//! Hand-rolled argument parsing for the `asset` binary.
//!
//! No `clap`: the crate has no argument-parsing dependency, all existing tool
//! binaries hand-roll `std::env::args`, and adding one would rebuild every
//! binary in a crate that already takes minutes. Revisit if the verb surface
//! passes roughly fifteen, where generated `--help` starts to earn it.
//!
//! ## Dependency rules
//! - Depends on the sibling verb modules for their option structs. Nothing else.

use std::path::PathBuf;

use crate::asset_tools::verb_art::ArtForOptions;
use crate::asset_tools::verb_compare::CompareOptions;
use crate::asset_tools::verb_csf::CsfOptions;
use crate::asset_tools::verb_extract::ExtractOptions;
use crate::asset_tools::verb_find::FindOptions;
use crate::asset_tools::verb_info::InfoOptions;
use crate::asset_tools::verb_ini::{IniOptions, IniQuery};
use crate::asset_tools::verb_ls::{LsOptions, SortKey};
use crate::asset_tools::verb_palette::PaletteForOptions;
use crate::asset_tools::verb_parse_check::ParseCheckOptions;
use crate::asset_tools::verb_render::RenderOptions;
use crate::asset_tools::verb_scan::{self, ScanOptions};
use crate::asset_tools::verb_sound::SoundOptions;

pub enum Verb {
    Find { name: String },
    Ls { archive: String },
    Info { name: String },
    Render { name: String },
    PaletteFor { name: String },
    Archives,
    Extract { name: String },
    CsfGet { key: String },
    CsfGrep { text: String },
    Sound { name: String },
    BagLs,
    ArtFor { type_id: String },
    Scan,
    ParseCheck,
    CorpusBaseline,
    Compare { name: String },
    IniGet { section: String, key: String },
    IniType { type_id: String },
    IniRoots,
    IniExpand { name: String },
    Help,
}

pub struct Cli {
    pub ra2_dir: Option<PathBuf>,
    pub all_mixes: bool,
    pub verb: Verb,
    pub find: FindOptions,
    pub ls: LsOptions,
    pub info: InfoOptions,
    pub render: RenderOptions,
    pub palette: PaletteForOptions,
    pub extract: ExtractOptions,
    pub csf: CsfOptions,
    pub sound: SoundOptions,
    pub art: ArtForOptions,
    /// `art-for --image`: the rules `Image=` value, when the caller knows it.
    pub art_image: String,
    pub scan: ScanOptions,
    pub parse_check: ParseCheckOptions,
    pub compare: CompareOptions,
    pub corpus_baseline_out: Option<PathBuf>,
    pub ini: IniOptions,
    pub ini_roots: Option<crate::util::ini_roots::IniRootConfig>,
    pub ini_strict: bool,
    pub ini_source_only: bool,
    pub ini_max_depth: usize,
}

pub fn usage() -> &'static str {
    "\
asset — headless browser for retail RA2/YR assets. Output is JSON on stdout.

USAGE
  asset <verb> [target] [options]

VERBS
  ini-get <SECTION> <KEY>  Inspect exact authored values and a production INI accessor.
  ini-type <TYPE>          Inspect final scenario ObjectType movement/flight fields.
  ini-roots               Inspect the selected Rules/Art/AI roots and byte sources.
  ini-expand <FILE>        Probe production Include/Inherits expansion and sources.
  find <NAME>          Which archive wins, what shadows it, and what is
                       catalogued but unreachable by name lookup.
  ls <ARCHIVE>         Paged listing of one archive's entries.
  info <NAME>          Parsed structure: SHP frame tables, TMP tiles, VXL limbs,
                       palettes, CSF/AUD/PCX/FNT/VPL headers.
  render <NAME>        Write PNGs. Handles SHP, TMP, PCX, PAL and VXL.
  palette-for <NAME>   Which palette applies, and the full reasoning chain.
  archives             Every mounted archive, with lookup reachability marked.
  extract <NAME>       Write an asset's raw bytes to disk.
  csf-get <KEY>        One string-table entry, with its normalisation flagged.
  csf-grep <TEXT>      Search keys and values of the string table.
  sound <NAME>         One audio-bag entry; --wav decodes it.
  bag-ls               List the audio bag, filtered by --prefix.
  art-for <TYPE>       Rules type id to the art files that back it, resolved.
  compare <NAME>       Every archive's copy of one name, side by side.
  scan                 Search every archive by format and field predicates.
  parse-check          Run every parser over the whole corpus; report failures.
  corpus-baseline      Export a complete candidate inventory and decoder ratchets.
                       Requires --all-mixes --out <NEW_FILE>; never overwrites.
                       Rust decoder baselines are not native parity evidence.

ini-get
  --domain rules|art  Required; ARTMD is fixed and rejects scenario options.
  --reader raw|int|bool|double|string|range|speed|coord
                       Required; accessor output is not a final gameplay field.
  --default <VALUE>   Required for typed readers (bool: true/false; coord: x,y,z).
  --capacity <N>      Required for string only; native byte capacity.
  --map <NAME/PATH>   Required with rules; production loose/MIX map selection.
  --mode-id <ID>      Required with rules; selected skirmish MPModesMD row.
                       --all-mixes is rejected. No campaign/extra TMCJ4F pass.

ini-type
  --domain rules     Required; final types belong to the scenario RuleSet.
  --map <NAME/PATH>  Required; same production source selection as ini-get.
  --mode-id <ID>     Required; selected skirmish MPModesMD row.
                      Reports source identities and complete Jumpjet type parameters,
                      including exact stored float/fixed bits. No runtime Link clamp.
                      --reader/--default/--capacity/--all-mixes are rejected.

ini-expand
  --source-only        INI-only resource root; no required retail game archives.
  --strict             Fail on the first reference diagnostic instead of skipping it.
  --max-depth <N>      Positive active reference-edge limit (default 64).
                       --all-mixes widens tooling lookup only, not game mounting.

GLOBAL OPTIONS
  --ini-profile <yr|mo|auto|custom>  ini-roots/get/type only; overrides [ini] as a unit.
  --rules-ini <NAME> --art-ini <NAME> --ai-ini <NAME>
                       Custom profile requires all three plain INI basenames.
  --ra2-dir <PATH>     Retail install root. Overrides $RA2_DIR and config.toml.
  --all-mixes          Also mount archives the game's startup path skips.
                       Tooling-only: hits found this way are not what the game
                       would resolve.
  -h, --help           This text.

find
  --all                Search every mounted archive (default).
  --winner-only        Skip the shadow/catalogue sweep.

ls
  --filter <SUBSTR>    Case-insensitive substring match on the resolved name.
  --format <TAG>       Keep only this sniffed format, e.g. shp, pal, tmp.
  --sort <KEY>         index (default) | name | size | hash
  --limit <N>          Page size. Default 100.
  --offset <N>         Rows to skip.

info
  --frame <N>          Frame selected by --ascii. Default 0.
  --ascii              Palette-index grid for the selected frame. Frames up to
                       4096 pixels only; more precise than an image and free.
  --limit <N>          Max frames/tiles listed. Default 64.

render
  --frame <N>          Render one frame (or one TMP tile). Default: all.
  --palette <NAME>     Force a palette, e.g. sidebar.pal.
  --house <N>          Apply the [Colors] scheme N to the [16,32) remap band.
  --crop               Draw the bare frame sub-rect instead of the full canvas.
                       The default hides no frame_x/frame_y placement.
  --scale <N>          Integer upscale. Default: fit the long edge to 256-1024.
  --limit <N>          Max frames rendered. Default 64.
  --out <DIR>          Output root. Default target/asset.
  --isometric          TMP: compose the template as it appears in game.
  --grid               TMP: lay tiles out in a labelled grid (default).
  --facing <0-255>     VXL: render this facing. Repeatable. Default: 8 compass
                       facings (0x00 N, 0x40 E, 0x80 S, 0xC0 W).
  --vpl <NAME>         VXL: voxel lighting table. Default voxels.vpl.
  --transparent-index <N>
                       PCX: palette index to treat as transparent.

palette-for
  --palette <NAME>     Test a specific palette against the inference chain.

csf-get / csf-grep
  --source <NAME>      Which .csf to read. Default: ra2md.csf then ra2.csf.
  --raw                Also report the stored text before normalisation.
  --limit/--offset     Paging. Default limit 50.

sound / bag-ls
  --bag <NAME>         Bag pair to open, without extension.
  --prefix <P>         bag-ls: name prefix filter.
  --wav                sound: decode and write a .wav.
  --limit/--offset     Paging. Default limit 100.

art-for
  --theater <T>        tem (default) | sno | urb | lun | des | ubn
  --image <ID>         The rules Image= value, when you already know it.

compare
  --frame <N>          Frame rendered from each variant. Default 0.
  --scale <N>          Shared integer upscale across all variants.
  --no-render          Structural diff only; write no PNGs.

scan
  --format <TAG>       Restrict to one format. Strongly recommended: without it
                       every entry in the corpus is sniffed and parsed.
  --archive <SUBSTR>   Restrict to archives whose name contains this.
  --where <K=V,...>    AND-only field predicates. Operators = >= <= > <.
                       Fields vary by format: shp has w, h, frames; tmp has
                       tw, th, tiles; vxl has limbs, voxels. An unknown field
                       is an error, never an empty result.
                       Example: --format shp --where w=60,frames>100
  --limit <N>          Page size. Default 200.
  --offset <N>         Rows to skip.

parse-check
  --format <TAG>       Restrict to one format.
  --limit <N>          Max sampled failures reported per format. Default 8.
"
}

/// Parse the command line. `argv` must already have the program name removed.
pub fn parse<I: IntoIterator<Item = String>>(argv: I) -> Result<Cli, String> {
    let mut cli = Cli {
        ra2_dir: None,
        all_mixes: false,
        verb: Verb::Help,
        find: FindOptions::default(),
        ls: LsOptions::default(),
        info: InfoOptions::default(),
        render: RenderOptions::default(),
        palette: PaletteForOptions::default(),
        extract: ExtractOptions::default(),
        csf: CsfOptions::default(),
        sound: SoundOptions::default(),
        art: ArtForOptions::default(),
        art_image: String::new(),
        scan: ScanOptions::default(),
        parse_check: ParseCheckOptions::default(),
        compare: CompareOptions::default(),
        corpus_baseline_out: None,
        ini: IniOptions::default(),
        ini_roots: None,
        ini_strict: false,
        ini_source_only: false,
        ini_max_depth: 64,
    };

    let mut args = argv.into_iter().peekable();
    let Some(verb_word) = args.next() else {
        return Ok(cli);
    };
    if matches!(verb_word.as_str(), "-h" | "--help" | "help") {
        return Ok(cli);
    }

    if matches!(
        verb_word.as_str(),
        "ini-get" | "ini-type" | "ini-roots" | "ini-expand"
    ) && args
        .peek()
        .is_some_and(|arg| matches!(arg.as_str(), "-h" | "--help"))
    {
        return Ok(cli);
    }

    // The corpus-wide verbs sweep everything and so take no positional target.
    let needs_target = !matches!(
        verb_word.as_str(),
        "archives" | "bag-ls" | "scan" | "parse-check" | "corpus-baseline" | "ini-roots"
    );
    let target = if needs_target {
        match args.peek() {
            Some(word) if !word.starts_with('-') => args.next(),
            _ => None,
        }
    } else {
        None
    };

    cli.verb = match verb_word.as_str() {
        "ini-roots" => Verb::IniRoots,
        "ini-get" => Verb::IniGet {
            section: require_target(target, "ini-get", "<SECTION> <KEY>")?,
            key: value(&mut args, "ini-get key")?,
        },
        "ini-type" => Verb::IniType {
            type_id: require_target(target, "ini-type", "<TYPE>")?,
        },
        "ini-expand" => Verb::IniExpand {
            name: require_target(target, "ini-expand", "<FILE>")?,
        },
        "find" => Verb::Find {
            name: require_target(target, "find", "<NAME>")?,
        },
        "ls" => Verb::Ls {
            archive: require_target(target, "ls", "<ARCHIVE>")?,
        },
        "info" => Verb::Info {
            name: require_target(target, "info", "<NAME>")?,
        },
        "render" => Verb::Render {
            name: require_target(target, "render", "<NAME>")?,
        },
        "palette-for" => Verb::PaletteFor {
            name: require_target(target, "palette-for", "<NAME>")?,
        },
        "archives" => Verb::Archives,
        "extract" => Verb::Extract {
            name: require_target(target, "extract", "<NAME>")?,
        },
        "csf-get" => Verb::CsfGet {
            key: require_target(target, "csf-get", "<KEY>")?,
        },
        "csf-grep" => Verb::CsfGrep {
            text: require_target(target, "csf-grep", "<TEXT>")?,
        },
        "sound" => Verb::Sound {
            name: require_target(target, "sound", "<NAME>")?,
        },
        "bag-ls" => Verb::BagLs,
        "art-for" => Verb::ArtFor {
            type_id: require_target(target, "art-for", "<TYPE>")?,
        },
        "compare" => Verb::Compare {
            name: require_target(target, "compare", "<NAME>")?,
        },
        "scan" => Verb::Scan,
        "parse-check" => Verb::ParseCheck,
        "corpus-baseline" => Verb::CorpusBaseline,
        other => return Err(format!("unknown verb \"{other}\"")),
    };

    while let Some(flag) = args.next() {
        if matches!(cli.verb, Verb::IniRoots)
            && !matches!(
                flag.as_str(),
                "--ra2-dir"
                    | "--all-mixes"
                    | "--ini-profile"
                    | "--rules-ini"
                    | "--art-ini"
                    | "--ai-ini"
                    | "-h"
                    | "--help"
            )
        {
            return Err(flag_not_valid(&flag, &cli.verb));
        }
        if matches!(cli.verb, Verb::IniExpand { .. })
            && !matches!(
                flag.as_str(),
                "--ra2-dir"
                    | "--all-mixes"
                    | "--strict"
                    | "--max-depth"
                    | "--source-only"
                    | "-h"
                    | "--help"
            )
        {
            return Err(flag_not_valid(&flag, &cli.verb));
        }
        if matches!(cli.verb, Verb::IniGet { .. } | Verb::IniType { .. })
            && !matches!(
                flag.as_str(),
                "--ra2-dir"
                    | "--ini-profile"
                    | "--rules-ini"
                    | "--art-ini"
                    | "--ai-ini"
                    | "--domain"
                    | "--reader"
                    | "--default"
                    | "--capacity"
                    | "--map"
                    | "--mode-id"
                    | "-h"
                    | "--help"
            )
        {
            return Err(flag_not_valid(&flag, &cli.verb));
        }
        if matches!(cli.verb, Verb::CorpusBaseline)
            && !matches!(
                flag.as_str(),
                "--all-mixes" | "--ra2-dir" | "--out" | "-h" | "--help"
            )
        {
            return Err(flag_not_valid(&flag, &cli.verb));
        }
        match flag.as_str() {
            "-h" | "--help" => {
                cli.verb = Verb::Help;
                return Ok(cli);
            }
            "--ra2-dir" => cli.ra2_dir = Some(PathBuf::from(value(&mut args, "--ra2-dir")?)),
            "--all-mixes" => cli.all_mixes = true,
            "--ini-profile" | "--rules-ini" | "--art-ini" | "--ai-ini" => {
                if !matches!(
                    cli.verb,
                    Verb::IniGet { .. } | Verb::IniType { .. } | Verb::IniRoots
                ) {
                    return Err(flag_not_valid(&flag, &cli.verb));
                }
                let config = cli.ini_roots.get_or_insert_with(Default::default);
                let text = value(&mut args, &flag)?;
                match flag.as_str() {
                    "--ini-profile" => config.profile = text.parse()?,
                    "--rules-ini" => config.rules = Some(text),
                    "--art-ini" => config.art = Some(text),
                    "--ai-ini" => config.ai = Some(text),
                    _ => unreachable!(),
                }
            }
            "--strict" | "--max-depth" | "--source-only" => {
                if !matches!(cli.verb, Verb::IniExpand { .. }) {
                    return Err(flag_not_valid(&flag, &cli.verb));
                }
                if flag == "--source-only" {
                    cli.ini_source_only = true;
                } else if flag == "--strict" {
                    cli.ini_strict = true;
                } else {
                    cli.ini_max_depth = number(&mut args, &flag)?;
                    if cli.ini_max_depth == 0 {
                        return Err("--max-depth must be positive".into());
                    }
                }
            }
            "--domain" | "--reader" | "--default" | "--capacity" | "--map" | "--mode-id" => {
                if !matches!(cli.verb, Verb::IniGet { .. } | Verb::IniType { .. }) {
                    return Err(flag_not_valid(&flag, &cli.verb));
                }
                match flag.as_str() {
                    "--domain" => cli.ini.domain = Some(value(&mut args, &flag)?),
                    "--reader" => cli.ini.reader = Some(value(&mut args, &flag)?),
                    "--default" => cli.ini.default = Some(value(&mut args, &flag)?),
                    "--capacity" => cli.ini.capacity = Some(number(&mut args, &flag)?),
                    "--map" => cli.ini.map = Some(value(&mut args, &flag)?),
                    "--mode-id" => cli.ini.mode_id = Some(number(&mut args, &flag)?),
                    _ => unreachable!(),
                }
            }
            // One output root serves every verb that writes files.
            "--out" => {
                let dir = PathBuf::from(value(&mut args, "--out")?);
                if matches!(cli.verb, Verb::CorpusBaseline) {
                    cli.corpus_baseline_out = Some(dir);
                    continue;
                }
                cli.render.out = dir.clone();
                cli.extract.out = dir.clone();
                cli.compare.out = dir.clone();
                cli.sound.out = dir;
            }

            "--all" => match cli.verb {
                Verb::Find { .. } => cli.find.all = true,
                // For render, --all is the default; accept it as an explicit no-op
                // so the obvious spelling does not error.
                Verb::Render { .. } => cli.render.frame = None,
                _ => return Err(flag_not_valid(&flag, &cli.verb)),
            },
            "--winner-only" => cli.find.all = false,

            "--filter" => cli.ls.filter = Some(value(&mut args, "--filter")?),
            "--format" => {
                let tag = value(&mut args, "--format")?;
                match cli.verb {
                    Verb::Scan => cli.scan.format = Some(tag),
                    Verb::ParseCheck => cli.parse_check.format = Some(tag),
                    _ => cli.ls.format = Some(tag),
                }
            }
            "--where" => {
                cli.scan.predicates = verb_scan::parse_predicates(&value(&mut args, "--where")?)?
            }
            "--archive" => cli.scan.archive = Some(value(&mut args, "--archive")?),
            "--sort" => {
                cli.ls.sort = match value(&mut args, "--sort")?.as_str() {
                    "index" => SortKey::Index,
                    "name" => SortKey::Name,
                    "size" => SortKey::Size,
                    "hash" => SortKey::Hash,
                    other => {
                        return Err(format!(
                            "--sort must be index, name, size or hash (got \"{other}\")"
                        ));
                    }
                }
            }
            "--offset" => {
                let n = number::<usize>(&mut args, "--offset")?;
                match cli.verb {
                    Verb::CsfGet { .. } | Verb::CsfGrep { .. } => cli.csf.offset = n,
                    Verb::Sound { .. } | Verb::BagLs => cli.sound.offset = n,
                    Verb::Scan => cli.scan.offset = n,
                    _ => cli.ls.offset = n,
                }
            }

            "--ascii" => cli.info.ascii = true,
            "--no-render" => cli.compare.no_render = true,
            "--crop" => cli.render.crop = true,

            // --- Phase 2 flags ---
            "--isometric" => cli.render.isometric = true,
            "--grid" => cli.render.isometric = false,
            "--facing" => cli
                .render
                .facings
                .push(number::<u8>(&mut args, "--facing")?),
            "--vpl" => cli.render.vpl = Some(value(&mut args, "--vpl")?),
            "--transparent-index" => {
                cli.render.transparent_index = Some(number::<u8>(&mut args, "--transparent-index")?)
            }
            "--theater" => cli.art.theater = value(&mut args, "--theater")?,
            "--image" => cli.art_image = value(&mut args, "--image")?,
            "--prefix" => cli.sound.prefix = Some(value(&mut args, "--prefix")?),
            "--wav" => cli.sound.wav = true,
            "--bag" => cli.sound.bag = Some(value(&mut args, "--bag")?),
            "--source" => cli.csf.source = Some(value(&mut args, "--source")?),
            "--raw" => cli.csf.raw = true,
            "--scale" => {
                let n = number::<u32>(&mut args, "--scale")?;
                cli.render.scale = Some(n);
                cli.compare.scale = Some(n);
            }
            "--house" => cli.render.house = Some(number::<u8>(&mut args, "--house")?),
            "--palette" => {
                let name = value(&mut args, "--palette")?;
                cli.render.palette = Some(name.clone());
                cli.palette.palette_override = Some(name);
            }

            // --frame and --limit mean different things per verb.
            "--frame" => {
                let n = number::<usize>(&mut args, "--frame")?;
                match cli.verb {
                    Verb::Info { .. } => cli.info.frame = n,
                    Verb::Render { .. } => cli.render.frame = Some(n),
                    Verb::Compare { .. } => cli.compare.frame = n,
                    _ => return Err(flag_not_valid(&flag, &cli.verb)),
                }
            }
            "--limit" => {
                let n = number::<usize>(&mut args, "--limit")?;
                match cli.verb {
                    Verb::Ls { .. } => cli.ls.limit = n,
                    Verb::Info { .. } => cli.info.limit = n,
                    Verb::Render { .. } => cli.render.limit = n,
                    Verb::CsfGet { .. } | Verb::CsfGrep { .. } => cli.csf.limit = n,
                    Verb::Sound { .. } | Verb::BagLs => cli.sound.limit = n,
                    Verb::Scan => cli.scan.limit = n,
                    // parse-check's limit caps sampled failures per format.
                    Verb::ParseCheck => cli.parse_check.failure_cap = n,
                    _ => return Err(flag_not_valid(&flag, &cli.verb)),
                }
            }

            other => return Err(format!("unrecognised option \"{other}\"")),
        }
    }

    match &cli.verb {
        Verb::IniGet { section, key } => cli.ini.validate(IniQuery::Accessor { section, key })?,
        Verb::IniType { type_id } => cli.ini.validate(IniQuery::Type { type_id })?,
        _ => {}
    }
    if matches!(cli.verb, Verb::CorpusBaseline) {
        if !cli.all_mixes {
            return Err("corpus-baseline requires --all-mixes: the reference includes archives outside startup reach".to_string());
        }
        if cli.corpus_baseline_out.is_none() {
            return Err(
                "corpus-baseline requires --out <NEW_FILE>; existing files are never overwritten"
                    .to_string(),
            );
        }
    }
    if let Some(config) = &cli.ini_roots {
        config.validate()?;
    }
    Ok(cli)
}

fn require_target(target: Option<String>, verb: &str, placeholder: &str) -> Result<String, String> {
    target.ok_or_else(|| format!("{verb} needs a target: asset {verb} {placeholder}"))
}

fn value<I: Iterator<Item = String>>(args: &mut I, flag: &str) -> Result<String, String> {
    args.next()
        .ok_or_else(|| format!("{flag} needs a value"))
        .and_then(|v| {
            if v.starts_with("--") {
                Err(format!("{flag} needs a value (got the flag \"{v}\")"))
            } else {
                Ok(v)
            }
        })
}

fn number<T: std::str::FromStr>(
    args: &mut impl Iterator<Item = String>,
    flag: &str,
) -> Result<T, String> {
    value(args, flag)?
        .parse::<T>()
        .map_err(|_| format!("{flag} needs a non-negative integer"))
}

fn flag_not_valid(flag: &str, verb: &Verb) -> String {
    let verb_name = match verb {
        Verb::Find { .. } => "find",
        Verb::Ls { .. } => "ls",
        Verb::Info { .. } => "info",
        Verb::Render { .. } => "render",
        Verb::PaletteFor { .. } => "palette-for",
        Verb::Archives => "archives",
        Verb::Extract { .. } => "extract",
        Verb::CsfGet { .. } => "csf-get",
        Verb::CsfGrep { .. } => "csf-grep",
        Verb::Sound { .. } => "sound",
        Verb::BagLs => "bag-ls",
        Verb::ArtFor { .. } => "art-for",
        Verb::Compare { .. } => "compare",
        Verb::Scan => "scan",
        Verb::ParseCheck => "parse-check",
        Verb::CorpusBaseline => "corpus-baseline",
        Verb::IniGet { .. } => "ini-get",
        Verb::IniType { .. } => "ini-type",
        Verb::IniRoots => "ini-roots",
        Verb::IniExpand { .. } => "ini-expand",
        Verb::Help => "help",
    };
    format!("{flag} is not valid for `{verb_name}`")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ini_roots_cli_keeps_profile_choice_whole_and_rejects_other_verbs() {
        let auto = parse(args(&["ini-roots", "--ini-profile", "auto", "--all-mixes"])).unwrap();
        assert!(matches!(auto.verb, Verb::IniRoots));
        assert_eq!(
            auto.ini_roots.unwrap().profile,
            crate::util::ini_roots::IniProfile::Auto
        );
        let custom = parse(args(&[
            "ini-roots",
            "--rules-ini",
            "r.ini",
            "--art-ini",
            "a.ini",
            "--ai-ini",
            "i.ini",
            "--ini-profile",
            "custom",
        ]))
        .unwrap();
        assert_eq!(
            custom.ini_roots.unwrap().resolve(|_| true).unwrap().names(),
            ["r.ini", "a.ini", "i.ini"]
        );
        for words in [
            vec![
                "ini-roots",
                "--ini-profile",
                "custom",
                "--rules-ini",
                "r.ini",
            ],
            vec!["ini-roots", "--ini-profile", "mo", "--ai-ini", "aimd.ini"],
            vec!["ini-roots", "--ini-profile", "unknown"],
            vec!["ini-roots", "--out", "somewhere"],
            vec!["find", "r.ini", "--ini-profile", "mo"],
            vec!["ini-expand", "r.ini", "--ini-profile", "mo"],
            vec!["ini-roots", "--strict"],
            vec!["ini-roots", "--source-only"],
        ] {
            assert!(parse(args(&words)).is_err(), "{words:?}");
        }
    }

    #[test]
    fn ini_expand_probe_admits_only_its_own_read_only_options() {
        let cli = parse(args(&[
            "ini-expand",
            "rulesmo.ini",
            "--strict",
            "--max-depth",
            "12",
            "--all-mixes",
            "--source-only",
        ]))
        .unwrap();
        assert!(matches!(cli.verb, Verb::IniExpand { name } if name == "rulesmo.ini"));
        assert!(cli.ini_strict && cli.all_mixes && cli.ini_source_only);
        assert_eq!(cli.ini_max_depth, 12);
        for words in [
            vec!["ini-expand"],
            vec!["ini-expand", "a.ini", "--max-depth", "0"],
            vec!["ini-expand", "a.ini", "--out", "a.json"],
            vec!["find", "a", "--strict"],
        ] {
            assert!(parse(args(&words)).is_err(), "{words:?}");
        }
    }

    fn args(words: &[&str]) -> Vec<String> {
        words.iter().map(|w| (*w).to_string()).collect()
    }

    #[test]
    fn corpus_baseline_requires_explicit_scope_and_destination() {
        assert!(parse(args(&["corpus-baseline"])).is_err());
        assert!(parse(args(&["corpus-baseline", "--all-mixes"])).is_err());
        assert!(parse(args(&["corpus-baseline", "--out", "new.json"])).is_err());
        assert!(
            parse(args(&[
                "corpus-baseline",
                "--all-mixes",
                "--out",
                "new.json",
                "--format",
                "shp"
            ]))
            .is_err()
        );
        let cli = parse(args(&[
            "corpus-baseline",
            "--all-mixes",
            "--out",
            "new.json",
        ]))
        .unwrap();
        assert!(matches!(cli.verb, Verb::CorpusBaseline));
        assert_eq!(
            cli.corpus_baseline_out.as_deref(),
            Some(std::path::Path::new("new.json"))
        );
    }

    #[test]
    fn missing_target_is_an_actionable_error() {
        let err = parse(args(&["info"]))
            .err()
            .expect("missing target is an error");
        assert!(err.contains("asset info <NAME>"), "got {err}");
    }

    #[test]
    fn limit_binds_to_the_active_verb() {
        let cli = parse(args(&["ls", "ra2.mix", "--limit", "5"])).expect("parses");
        assert_eq!(cli.ls.limit, 5);
        let cli = parse(args(&["render", "x.shp", "--limit", "5"])).expect("parses");
        assert_eq!(cli.render.limit, 5);
    }

    #[test]
    fn palette_flag_reaches_both_consumers() {
        let cli = parse(args(&["render", "x.shp", "--palette", "sidebar.pal"])).expect("parses");
        assert_eq!(cli.render.palette.as_deref(), Some("sidebar.pal"));
        assert_eq!(cli.palette.palette_override.as_deref(), Some("sidebar.pal"));
    }

    #[test]
    fn a_flag_swallowing_the_next_flag_is_rejected() {
        let err = parse(args(&["ls", "ra2.mix", "--filter", "--limit"]))
            .err()
            .expect("a flag as a value is an error");
        assert!(err.contains("--filter needs a value"), "got {err}");
    }

    #[test]
    fn unknown_verb_and_unknown_flag_both_report() {
        assert!(parse(args(&["frobnicate"])).is_err());
        assert!(parse(args(&["find", "x", "--nope"])).is_err());
    }

    #[test]
    fn a_path_containing_cd_does_not_break_parsing() {
        // The manager pins its media mode precisely because the native default
        // substring-matches "-CD" across argv; make sure the parser is clean too.
        let cli = parse(args(&["find", "x.shp", "--ra2-dir", "D:/games-CD/ra2"])).expect("parses");
        assert_eq!(
            cli.ra2_dir.as_deref(),
            Some(std::path::Path::new("D:/games-CD/ra2"))
        );
    }
    #[test]
    fn ini_query_requires_explicit_domain_reader_and_scenario() {
        let cli = parse(args(&[
            "ini-get",
            "General",
            "TreeStrength",
            "--domain",
            "rules",
            "--reader",
            "int",
            "--default",
            "-1",
            "--map",
            "Dustbowl.mmx",
            "--mode-id",
            "1",
        ]))
        .unwrap();
        assert!(
            matches!(cli.verb, Verb::IniGet { section, key } if section == "General" && key == "TreeStrength")
        );
        assert_eq!(cli.ini.default.as_deref(), Some("-1"));
        for words in [
            vec!["ini-get", "S", "K"],
            vec!["ini-get", "S", "K", "--domain", "rules", "--reader", "raw"],
            vec!["ini-get", "S", "K", "--domain", "art", "--reader", "int"],
            vec![
                "ini-get", "S", "K", "--domain", "art", "--reader", "raw", "--map", "x",
            ],
            vec![
                "ini-get",
                "S",
                "K",
                "--domain",
                "art",
                "--reader",
                "raw",
                "--all-mixes",
            ],
            vec![
                "ini-get",
                "S",
                "K",
                "--domain",
                "art",
                "--reader",
                "bool",
                "--default",
                "yes",
            ],
            vec![
                "ini-get",
                "S",
                "K",
                "--domain",
                "art",
                "--reader",
                "string",
                "--default",
                "x",
            ],
            vec![
                "ini-get",
                "S",
                "K",
                "--domain",
                "art",
                "--reader",
                "raw",
                "--capacity",
                "4",
            ],
            vec!["find", "x", "--domain", "art"],
        ] {
            assert!(parse(args(&words)).is_err(), "accepted {words:?}");
        }
        assert!(matches!(
            parse(args(&["ini-get", "--help"])).unwrap().verb,
            Verb::Help
        ));
    }

    #[test]
    fn ini_string_capacity_and_coord_defaults_are_explicit_cli_values() {
        let cli = parse(args(&[
            "ini-get",
            "ART",
            "Key",
            "--domain",
            "art",
            "--reader",
            "string",
            "--default",
            "",
            "--capacity",
            "128",
        ]))
        .unwrap();
        assert_eq!(cli.ini.capacity, Some(128));
        assert!(
            parse(args(&[
                "ini-get",
                "ART",
                "Key",
                "--domain",
                "art",
                "--reader",
                "coord",
                "--default",
                "-1,0,3"
            ]))
            .is_ok()
        );
        assert!(
            parse(args(&[
                "ini-get",
                "ART",
                "Key",
                "--domain",
                "art",
                "--reader",
                "coord",
                "--default",
                "1,2"
            ]))
            .is_err()
        );
    }

    #[test]
    fn ini_type_requires_rules_scenario_and_rejects_accessor_options() {
        let cli = parse(args(&[
            "ini-type",
            "SHAD",
            "--domain",
            "rules",
            "--map",
            "XMP03T4.MAP",
            "--mode-id",
            "1",
        ]))
        .unwrap();
        assert!(matches!(cli.verb, Verb::IniType { type_id } if type_id == "SHAD"));
        assert!(cli.ini.reader.is_none());
        assert_eq!(cli.ini.map.as_deref(), Some("XMP03T4.MAP"));
        assert_eq!(cli.ini.mode_id, Some(1));
        for words in [
            vec!["ini-type"],
            vec!["ini-type", "SHAD"],
            vec!["ini-type", "SHAD", "--domain", "art"],
            vec!["ini-type", "SHAD", "--domain", "rules"],
            vec!["ini-type", "SHAD", "--domain", "rules", "--map", "x"],
            vec!["ini-type", "SHAD", "--domain", "rules", "--mode-id", "1"],
        ] {
            assert!(parse(args(&words)).is_err(), "accepted {words:?}");
        }
        for extra in [
            vec!["--reader", "raw"],
            vec!["--default", "false"],
            vec!["--capacity", "128"],
            vec!["--all-mixes"],
            vec!["--out", "somewhere"],
        ] {
            let mut words = vec![
                "ini-type",
                "SHAD",
                "--domain",
                "rules",
                "--map",
                "x",
                "--mode-id",
                "1",
            ];
            words.extend(extra);
            assert!(parse(args(&words)).is_err(), "accepted {words:?}");
        }
        assert!(matches!(
            parse(args(&["ini-type", "--help"])).unwrap().verb,
            Verb::Help
        ));
    }
}
