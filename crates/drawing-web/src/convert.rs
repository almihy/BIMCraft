//! Drawings written as another format, for Arqiflow's server, which runs no DWG converter of its own (no LibreDWG, no ODA):
//! a DXF it made (the engine's site plan) written as a DWG that AutoCAD opens, and a DWG read as DXF for what on the server
//! still reads DXF (a survey's spot levels and contours, a drawing set's sheets).
//!
//! - DWG written: the drawing read into CADCraft's drawing (`cadcraft_index::read`) and written by `cadcraft_io`, whose DWG
//!   writer is acadrust's own (the one AutoCAD opens), never through DXF text.
//! - DWG read as DXF: acadrust's document written by acadrust's own DXF writer, with no CADCraft drawing in between, so
//!   what CADCraft does not model stays: a survey's contours drawn as splines keep their heights (CADCraft's splines are
//!   flat). The size is checked before the decoder runs, and a read that skipped the file's objects is refused, as
//!   `cadcraft-dwg` reads.
//!
//! Worked in the person's own Arqiflow (web/lib/compute/worker.ts, the jobs `drawing.dwg` and `drawing.dxf`).

use cadcraft_io::{SaveOptions, Version};
use wasm_bindgen::prelude::*;

use crate::err;

/// The DWG versions written, by AutoCAD's name for them and the code in the file: 2000 to 2018.
const VERSIONS: [(&str, &str); 6] =
    [("2000", "AC1015"), ("2004", "AC1018"), ("2007", "AC1021"), ("2010", "AC1024"), ("2013", "AC1027"), ("2018", "AC1032")];

fn version(asked: &str) -> Result<Version, String> {
    let asked = asked.trim();
    let code = if asked.is_empty() {
        "AC1032"
    } else {
        VERSIONS
            .iter()
            .find(|(year, code)| asked == *year || asked.eq_ignore_ascii_case(code))
            .map(|(_, code)| *code)
            .ok_or_else(|| format!("DWG version `{asked}` is not one CADCraft writes (2000, 2004, 2007, 2010, 2013 or 2018)"))?
    };
    Version::for_acadver(code).ok_or_else(|| format!("DWG version `{code}` is not known"))
}

/// A drawing's `bytes` (a DXF or a DWG; `name` says which by its extension, and a DWG is known by its first bytes too)
/// written as a DWG of `version` ("2018", "2013", ... "2000", or its code "AC1032"; empty: 2018).
#[wasm_bindgen]
pub fn dwg_of(bytes: &[u8], name: &str, version: &str) -> Result<Vec<u8>, JsError> {
    dwg_bytes(bytes, name, version).map_err(err)
}

/// A drawing's `bytes` (a DWG or a DXF) written as ASCII DXF, in the drawing's own version.
#[wasm_bindgen]
pub fn dxf_of(bytes: &[u8], name: &str) -> Result<Vec<u8>, JsError> {
    dxf_bytes(bytes, name).map_err(err)
}

pub(crate) fn dwg_bytes(bytes: &[u8], name: &str, asked: &str) -> Result<Vec<u8>, String> {
    let v = version(asked)?;
    let d = cadcraft_index::read(bytes, name)?;
    cadcraft_io::write_with(&d, "drawing.dwg", &SaveOptions { version: Some(v), binary: false }).map_err(|e| e.to_string())
}

pub(crate) fn dxf_bytes(bytes: &[u8], name: &str) -> Result<Vec<u8>, String> {
    use cadcraft_dwg::acadrust::{DwgReadOptions, DwgReader, DxfWriter};

    if !cadcraft_dwg::is_dwg(bytes) {
        return Err(format!("{name} is not a DWG"));
    }
    let most = cadcraft_dwg::Limits::DEFAULT.max_dwg_bytes;
    if bytes.len() > most {
        return Err(format!("DWG: the file is {} MB, larger than the {} MB CADCraft opens", bytes.len() >> 20, most >> 20));
    }
    let read = DwgReader::from_stream_with_options(std::io::Cursor::new(bytes), DwgReadOptions::failsafe())
        .read_with_stats()
        .map_err(|e| format!("DWG: {e}"))?;
    // Failsafe skips damaged records, not whole files: when the objects themselves couldn't be read, nothing was.
    if !read.stats.record_stream_read {
        let why = read.stats.diagnostics.first().map(|d| d.message.trim_start_matches("Failsafe: ").to_string());
        return Err(format!("DWG: {}", why.unwrap_or_else(|| "the file's objects couldn't be read".to_string())));
    }
    DxfWriter::new(&read.document).write_to_vec().map_err(|e| format!("DXF: {e}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A small DXF as the engine writes a site plan: a closed plot, a line and a text on two layers.
    const PLAN: &str = "0\nSECTION\n2\nHEADER\n9\n$ACADVER\n1\nAC1015\n9\n$INSUNITS\n70\n6\n0\nENDSEC\n0\nSECTION\n2\nTABLES\n0\nTABLE\n2\nLAYER\n70\n2\n0\nLAYER\n2\nPLOT\n70\n0\n62\n1\n6\nCONTINUOUS\n0\nLAYER\n2\nTEXT\n70\n0\n62\n7\n6\nCONTINUOUS\n0\nENDTAB\n0\nENDSEC\n0\nSECTION\n2\nENTITIES\n0\nLWPOLYLINE\n8\nPLOT\n90\n4\n70\n1\n10\n0.0\n20\n0.0\n10\n30.0\n20\n0.0\n10\n30.0\n20\n20.0\n10\n0.0\n20\n20.0\n0\nLINE\n8\nPLOT\n10\n0.0\n20\n0.0\n30\n0.0\n11\n30.0\n21\n20.0\n31\n0.0\n0\nTEXT\n8\nTEXT\n10\n5.0\n20\n5.0\n30\n0.0\n40\n1.0\n1\nSite plan\n0\nENDSEC\n0\nEOF\n";

    fn kinds(d: &cadcraft_doc::Drawing) -> Vec<String> {
        let mut k: Vec<String> = d.model.iter().map(|e| format!("{}@{}", cadcraft_index::kind_of(&e.kind), e.common.layer)).collect();
        k.sort();
        k
    }

    #[test]
    fn a_dxf_is_written_as_a_dwg_that_reads_back_the_same() {
        let dwg = dwg_bytes(PLAN.as_bytes(), "plan.dxf", "").expect("written");
        assert!(cadcraft_dwg::is_dwg(&dwg));
        assert_eq!(cadcraft_dwg::version(&dwg).as_deref(), Some("AC1032"));
        let back = cadcraft_index::read(&dwg, "plan.dwg").expect("read back");
        let before = cadcraft_index::read(PLAN.as_bytes(), "plan.dxf").expect("read");
        assert_eq!(kinds(&back), kinds(&before));
        assert_eq!(kinds(&back).len(), 3);
        assert!(["PLOT", "TEXT"].iter().all(|n| back.layers.iter().any(|l| l.name == *n)));
    }

    #[test]
    fn each_version_is_written_by_its_year_or_code() {
        for (year, code) in VERSIONS {
            let dwg = dwg_bytes(PLAN.as_bytes(), "plan.dxf", year).expect(year);
            assert_eq!(cadcraft_dwg::version(&dwg).as_deref(), Some(code));
        }
        assert!(dwg_bytes(PLAN.as_bytes(), "plan.dxf", "AC1015").is_ok());
        assert!(dwg_bytes(PLAN.as_bytes(), "plan.dxf", "R14").unwrap_err().contains("not one CADCraft writes"));
    }

    #[test]
    fn a_dwg_is_read_as_dxf() {
        let dwg = dwg_bytes(PLAN.as_bytes(), "plan.dxf", "2018").expect("written");
        let dxf = dxf_bytes(&dwg, "plan.dwg").expect("as dxf");
        let text = String::from_utf8_lossy(&dxf);
        assert!(text.contains("LWPOLYLINE") && text.contains("Site plan"));
        let back = cadcraft_index::read(&dxf, "plan.dxf").expect("read back");
        assert_eq!(kinds(&back).len(), 3);
    }

    /// A survey's contour drawn as a spline at its height (442): kept when its DWG is read as DXF, where CADCraft's own
    /// drawing would make it flat.
    const CONTOUR: &str = "0
SECTION
2
ENTITIES
0
SPLINE
5
2A
8
CONTOURS
70
8
71
3
72
8
73
4
74
0
40
0.0
40
0.0
40
0.0
40
0.0
40
1.0
40
1.0
40
1.0
40
1.0
10
0.0
20
0.0
30
442.0
10
10.0
20
5.0
30
442.0
10
20.0
20
5.0
30
442.0
10
30.0
20
0.0
30
442.0
0
ENDSEC
0
EOF
";

    #[test]
    fn a_dwg_read_as_dxf_keeps_what_cadcraft_does_not_model() {
        use cadcraft_dwg::acadrust::{DwgWriter, DxfReader};
        let doc = DxfReader::from_reader(std::io::Cursor::new(CONTOUR.as_bytes().to_vec())).expect("dxf").read().expect("read");
        let dwg = DwgWriter::write_to_vec(&doc).expect("dwg");
        let dxf = String::from_utf8(dxf_bytes(&dwg, "survey.dwg").expect("as dxf")).expect("text");
        let lines: Vec<&str> = dxf.lines().map(str::trim).collect();
        let pairs: Vec<(&str, &str)> = lines.chunks(2).filter(|p| p.len() == 2).map(|p| (p[0], p[1])).collect();
        let at = pairs.iter().position(|p| *p == ("0", "SPLINE")).expect("the spline");
        let heights: Vec<f64> =
            pairs[at + 1..].iter().take_while(|p| p.0 != "0").filter(|p| p.0 == "30").filter_map(|p| p.1.parse().ok()).collect();
        assert_eq!(heights, vec![442.0; 4], "{dxf}");
    }

    #[test]
    fn what_is_not_a_drawing_is_refused() {
        assert!(dwg_bytes(b"not a drawing at all", "x.dxf", "").is_err());
        assert!(dxf_bytes(b"AC1032 but cut short", "x.dwg").is_err());
    }
}
