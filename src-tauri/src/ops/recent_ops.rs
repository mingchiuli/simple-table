use std::io::Cursor;

use base64::Engine;
use calamine::{Data, Reader, Xlsx, Xls, Ods};
use csv::ReaderBuilder;
use image::{ImageBuffer, Rgba};
use serde::{Deserialize, Serialize};
use tauri::AppHandle;
use tauri_plugin_store::StoreExt;

// ==================== Types ====================

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RecentFile {
    pub id: String,
    pub path: String,
    pub file_name: String,
    pub last_opened: i64,
    pub file_size: i64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub thumbnail: Option<String>,
}

impl RecentFile {
    pub fn new(path: String, file_name: String, file_size: i64) -> Self {
        Self {
            id: uuid::Uuid::new_v4().to_string(),
            path,
            file_name,
            last_opened: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_millis() as i64,
            file_size,
            thumbnail: None,
        }
    }
}

// ==================== Store ====================

const STORE_FILE: &str = "recent-files.json";
const STORE_KEY: &str = "recent_files";
const MAX_RECENT: usize = 10;

pub struct RecentStore;

impl RecentStore {
    pub fn get_all(app: &AppHandle) -> Vec<RecentFile> {
        let store = match app.store(STORE_FILE) {
            Ok(s) => s,
            Err(_) => return Vec::new(),
        };
        store.get(STORE_KEY)
            .and_then(|v| serde_json::from_value(v.clone()).ok())
            .unwrap_or_default()
    }

    pub fn save(app: &AppHandle, files: &[RecentFile]) -> Result<(), String> {
        let store = app.store(STORE_FILE).map_err(|e| e.to_string())?;
        store.set(STORE_KEY, serde_json::to_value(files).map_err(|e| e.to_string())?);
        store.save().map_err(|e| e.to_string())?;
        Ok(())
    }

    pub fn add(app: &AppHandle, file: RecentFile) -> Result<RecentFile, String> {
        let mut files = Self::get_all(app);

        let existing_idx = files.iter().position(|f| f.path == file.path);
        if let Some(idx) = existing_idx {
            files[idx].last_opened = file.last_opened;
            files[idx].file_size = file.file_size;
            files[idx].thumbnail = file.thumbnail;
            files.sort_by(|a, b| b.last_opened.cmp(&a.last_opened));
            Self::save(app, &files)?;
            return Ok(files[idx].clone());
        }

        files.insert(0, file);
        files.truncate(MAX_RECENT);

        Self::save(app, &files)?;
        Ok(files[0].clone())
    }

    pub fn remove(app: &AppHandle, id: &str) -> Result<(), String> {
        let mut files = Self::get_all(app);
        files.retain(|f| f.id != id);
        Self::save(app, &files)
    }

    pub fn update_path(app: &AppHandle, id: &str, new_path: &str) -> Result<(), String> {
        let mut files = Self::get_all(app);

        let file = files
            .iter_mut()
            .find(|f| f.id == id)
            .ok_or("File not found")?;

        file.path = new_path.to_string();

        if let Some(name) = std::path::Path::new(new_path).file_name() {
            file.file_name = name.to_string_lossy().to_string();
        }

        files.sort_by(|a, b| b.last_opened.cmp(&a.last_opened));

        Self::save(app, &files)
    }

    pub fn exists(path: &str) -> bool {
        if path.starts_with("content://") || path.starts_with("file://") || path.starts_with("blob:") {
            return true;
        }
        std::path::Path::new(path).exists()
    }
}

// ==================== Thumbnail ====================

const THUMBNAIL_WIDTH: u32 = 200;
const CELL_WIDTH: u32 = 40;
const CELL_HEIGHT: u32 = 20;
const MAX_ROWS: usize = 10;
const MAX_COLS: usize = 10;

fn get_format_from_extension(ext: &str) -> Option<&'static str> {
    match ext.to_lowercase().as_str() {
        "xlsx" => Some("xlsx"),
        "xls" => Some("xls"),
        "ods" => Some("ods"),
        "csv" => Some("csv"),
        _ => None,
    }
}

pub fn generate_thumbnail_from_bytes(bytes: &[u8], extension: &str) -> Option<String> {
    let format = get_format_from_extension(extension)?;

    let rows = match format {
        "xlsx" => read_xlsx_from_bytes(bytes)?,
        "xls" => read_xls_from_bytes(bytes)?,
        "ods" => read_ods_from_bytes(bytes)?,
        "csv" => read_csv_from_bytes(bytes)?,
        _ => return None,
    };

    if rows.is_empty() {
        return None;
    }

    let num_cols = rows[0].len() as u32;
    let num_rows = rows.len() as u32;

    let width = THUMBNAIL_WIDTH.max(num_cols * CELL_WIDTH);
    let height = num_rows * CELL_HEIGHT;

    let mut img: ImageBuffer<Rgba<u8>, Vec<u8>> = ImageBuffer::new(width, height);

    for pixel in img.pixels_mut() {
        *pixel = Rgba([255, 255, 255, 255]);
    }

    for (row_idx, row) in rows.iter().enumerate() {
        for (col_idx, cell) in row.iter().enumerate() {
            let x = (col_idx as u32) * CELL_WIDTH;
            let y = (row_idx as u32) * CELL_HEIGHT;

            let bg_color = get_cell_color(cell);
            fill_rect(&mut img, x + 1, y + 1, CELL_WIDTH - 2, CELL_HEIGHT - 2, bg_color);
        }
    }

    let mut buffer = Cursor::new(Vec::new());
    img.write_to(&mut buffer, image::ImageFormat::Png).ok()?;

    let base64_str = base64::engine::general_purpose::STANDARD.encode(buffer.into_inner());
    Some(format!("data:image/png;base64,{}", base64_str))
}

fn read_xlsx_from_bytes(bytes: &[u8]) -> Option<Vec<Vec<Data>>> {
    let cursor = Cursor::new(bytes.to_vec());
    let mut workbook: Xlsx<std::io::BufReader<Cursor<Vec<u8>>>> =
        Xlsx::new(std::io::BufReader::new(cursor)).ok()?;
    read_sheet_data_from_bytes(&mut workbook)
}

fn read_xls_from_bytes(bytes: &[u8]) -> Option<Vec<Vec<Data>>> {
    let cursor = Cursor::new(bytes.to_vec());
    let mut workbook: Xls<std::io::BufReader<Cursor<Vec<u8>>>> =
        Xls::new(std::io::BufReader::new(cursor)).ok()?;
    read_sheet_data_xls_from_bytes(&mut workbook)
}

fn read_ods_from_bytes(bytes: &[u8]) -> Option<Vec<Vec<Data>>> {
    let cursor = Cursor::new(bytes.to_vec());
    let mut workbook: Ods<std::io::BufReader<Cursor<Vec<u8>>>> =
        Ods::new(std::io::BufReader::new(cursor)).ok()?;
    read_sheet_data_ods_from_bytes(&mut workbook)
}

fn read_csv_from_bytes(bytes: &[u8]) -> Option<Vec<Vec<Data>>> {
    let cursor = Cursor::new(bytes.to_vec());
    let mut reader = ReaderBuilder::new()
        .has_headers(false)
        .from_reader(cursor);

    let mut rows = Vec::new();
    for result in reader.records() {
        let record = result.ok()?;
        let row: Vec<Data> = record
            .iter()
            .map(|field| {
                if field.is_empty() {
                    Data::Empty
                } else if let Ok(int_val) = field.parse::<i64>() {
                    Data::Int(int_val)
                } else if let Ok(float_val) = field.parse::<f64>() {
                    Data::Float(float_val)
                } else if field.to_lowercase() == "true" {
                    Data::Bool(true)
                } else if field.to_lowercase() == "false" {
                    Data::Bool(false)
                } else {
                    Data::String(field.to_string())
                }
            })
            .collect();
        rows.push(row);
        if rows.len() >= MAX_ROWS {
            break;
        }
    }

    Some(rows)
}

fn read_sheet_data_from_bytes(workbook: &mut Xlsx<std::io::BufReader<Cursor<Vec<u8>>>>) -> Option<Vec<Vec<Data>>> {
    let sheets = workbook.sheet_names().to_owned();
    if sheets.is_empty() {
        return None;
    }

    let sheet_name = &sheets[0];
    let range = workbook.worksheet_range(sheet_name).ok()?;

    Some(
        range
            .rows()
            .take(MAX_ROWS)
            .map(|row| row.iter().take(MAX_COLS).cloned().collect())
            .collect(),
    )
}

fn read_sheet_data_xls_from_bytes(workbook: &mut Xls<std::io::BufReader<Cursor<Vec<u8>>>>) -> Option<Vec<Vec<Data>>> {
    let sheets = workbook.sheet_names().to_owned();
    if sheets.is_empty() {
        return None;
    }

    let sheet_name = &sheets[0];
    let range = workbook.worksheet_range(sheet_name).ok()?;

    Some(
        range
            .rows()
            .take(MAX_ROWS)
            .map(|row| row.iter().take(MAX_COLS).cloned().collect())
            .collect(),
    )
}

fn read_sheet_data_ods_from_bytes(workbook: &mut Ods<std::io::BufReader<Cursor<Vec<u8>>>>) -> Option<Vec<Vec<Data>>> {
    let sheets = workbook.sheet_names().to_owned();
    if sheets.is_empty() {
        return None;
    }

    let sheet_name = &sheets[0];
    let range = workbook.worksheet_range(sheet_name).ok()?;

    Some(
        range
            .rows()
            .take(MAX_ROWS)
            .map(|row| row.iter().take(MAX_COLS).cloned().collect())
            .collect(),
    )
}

fn fill_rect(img: &mut ImageBuffer<Rgba<u8>, Vec<u8>>, x: u32, y: u32, w: u32, h: u32, color: Rgba<u8>) {
    for j in y..(y + h).min(img.height()) {
        for i in x..(x + w).min(img.width()) {
            img.put_pixel(i, j, color);
        }
    }
}

fn get_cell_color(cell: &Data) -> Rgba<u8> {
    match cell {
        Data::Empty => Rgba([245, 245, 245, 255]),
        Data::Int(_) => Rgba([230, 242, 255, 255]),
        Data::Float(_) => Rgba([230, 242, 255, 255]),
        Data::String(s) => {
            if s.starts_with("http") || s.starts_with("www") {
                Rgba([255, 240, 230, 255])
            } else {
                Rgba([240, 255, 240, 255])
            }
        }
        Data::Bool(_) => Rgba([255, 250, 230, 255]),
        Data::DateTime(_) => Rgba([240, 240, 255, 255]),
        Data::DateTimeIso(_) => Rgba([240, 240, 255, 255]),
        Data::DurationIso(_) => Rgba([240, 240, 255, 255]),
        Data::Error(_) => Rgba([255, 230, 230, 255]),
    }
}

// ==================== Operations ====================

pub fn do_get_recent_files(app: &AppHandle) -> Vec<RecentFile> {
    RecentStore::get_all(app)
}

pub fn do_add_recent_file_with_thumbnail(
    app: &AppHandle,
    path: String,
    file_name: String,
    file_size: i64,
    bytes: Vec<u8>,
    extension: String,
) -> Result<RecentFile, String> {
    let mut recent_file = RecentFile::new(path, file_name, file_size);

    if let Some(thumbnail) = generate_thumbnail_from_bytes(&bytes, &extension) {
        recent_file.thumbnail = Some(thumbnail);
    }

    RecentStore::add(app, recent_file)
}

pub fn do_remove_recent_file(app: &AppHandle, id: String) -> Result<(), String> {
    RecentStore::remove(app, &id)
}

pub fn do_check_file_exists(path: String) -> bool {
    RecentStore::exists(&path)
}

pub fn do_update_recent_file_path(app: &AppHandle, id: String, new_path: String) -> Result<(), String> {
    RecentStore::update_path(app, &id, &new_path)
}
