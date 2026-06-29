use std::sync::Arc;
use std::time::{Duration, Instant};

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use serde_json::Value;
use tokio::sync::mpsc::{unbounded_channel, UnboundedReceiver, UnboundedSender};

use sncore::{SchemaCache, SnTableMeta};

use crate::client::{Client, HealthInfo};
use crate::column_config::ColumnConfig;
use crate::config::{Config, CustomList};
use crate::tables::{self, Category, ColumnDef, TableDef};

// ── Enums ────────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, PartialEq)]
pub enum BaseView {
    TableBrowser,
    AllTablesBrowser,
    RecordList,
    RecordDetail,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Overlay {
    ScriptRunner,
    Help,
    ColumnPicker,
}

// ── Column picker ─────────────────────────────────────────────────────────────

#[derive(Debug, Clone)]
pub struct ColPickerEntry {
    pub field: String,
    pub label: String,
    pub active: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub enum InputMode {
    Normal,
    Filter,
    Command,
    Script,
}

// ── Background messages ───────────────────────────────────────────────────────

pub enum AppMsg {
    HealthUpdate(HealthInfo),
    RecordsLoaded { table: String, records: Vec<Value> },
    RecordLoaded { record: Value },
    AllTablesLoaded(Vec<(String, String)>),
    SchemaLoaded { table: String, meta: SnTableMeta },
    ScriptResult(Result<String, String>),
    Error(String),
}

// ── Browser item list ─────────────────────────────────────────────────────────

#[derive(Debug, Clone)]
pub enum BrowserItem {
    Header(String),
    CustomList { name: String, table: String, query: String, order: String, columns: Vec<String> },
    Table { name: String, label: String },
    BrowseAll,
}

impl BrowserItem {
    pub fn is_selectable(&self) -> bool {
        !matches!(self, BrowserItem::Header(_))
    }
}

// ── App ───────────────────────────────────────────────────────────────────────

pub struct App {
    pub client: Arc<Client>,
    pub health: HealthInfo,
    last_health: Instant,

    // navigation
    pub base_view: BaseView,
    view_stack: Vec<BaseView>,
    pub overlay: Option<Overlay>,
    pub mode: InputMode,
    pub should_quit: bool,

    // table browser
    pub categories: Vec<Category>,
    pub browser_items: Vec<BrowserItem>,
    pub browser_cursor: usize,

    // all-tables browser
    pub all_tables: Vec<(String, String)>,
    pub all_tables_cursor: usize,
    pub all_tables_loading: bool,
    pub all_tables_filter: String,

    // record list
    pub current_table: String,
    pub current_table_def: Option<TableDef>,
    pub record_list_title: String,  // display name (table label or custom list name)
    pub records: Vec<Value>,
    pub record_cursor: usize,
    pub record_scroll: usize,
    pub records_loading: bool,
    pub record_filter: String,

    // input buffers
    pub filter_buf: String,
    pub command_buf: String,

    // schema
    pub current_schema: Option<SnTableMeta>,
    schema_cache: SchemaCache,

    // record detail
    pub detail_record: Option<Value>,
    pub detail_field_keys: Vec<String>,   // ordered field names for current detail record
    pub detail_table: String,
    pub detail_sys_id: String,
    pub detail_field_cursor: usize,
    pub detail_loading: bool,
    /// Stack of (table, sys_id) for Esc-back when following references
    pub detail_history: Vec<(String, String)>,

    // script runner
    pub script_buf: String,
    pub script_cursor: usize,
    pub script_output: Vec<String>,
    pub script_running: bool,
    pub script_out_scroll: usize,

    // column config & display options
    pub column_cfg: ColumnConfig,
    pub display_names: bool,
    /// Some(name) when viewing a named custom list; None for plain table views.
    pub current_list_name: Option<String>,
    /// Columns specified in the static sntui.toml for the current custom list.
    current_list_static_columns: Vec<String>,

    // column picker overlay state
    pub col_picker_fields: Vec<ColPickerEntry>,
    pub col_picker_cursor: usize,

    // status bar
    pub status: Option<String>,
    pub status_is_error: bool,

    // channels
    pub msg_tx: UnboundedSender<AppMsg>,
    msg_rx: UnboundedReceiver<AppMsg>,
}

impl App {
    pub fn new(port: u16, cfg: Config) -> Self {
        let (msg_tx, msg_rx) = unbounded_channel();
        let categories = tables::make_categories();
        let browser_items = build_browser_items(&categories, &cfg.lists);
        let browser_cursor = first_selectable(&browser_items, 0);

        Self {
            client: Arc::new(Client::new(port)),
            health: HealthInfo::default(),
            last_health: Instant::now() - Duration::from_secs(10),

            base_view: BaseView::TableBrowser,
            view_stack: Vec::new(),
            overlay: None,
            mode: InputMode::Normal,
            should_quit: false,

            categories,
            browser_items,
            browser_cursor,

            all_tables: Vec::new(),
            all_tables_cursor: 0,
            all_tables_loading: false,
            all_tables_filter: String::new(),

            current_table: String::new(),
            current_table_def: None,
            record_list_title: String::new(),
            current_schema: None,
            schema_cache: SchemaCache::new(),
            records: Vec::new(),
            record_cursor: 0,
            record_scroll: 0,
            records_loading: false,
            record_filter: String::new(),

            filter_buf: String::new(),
            command_buf: String::new(),

            detail_record: None,
            detail_field_keys: Vec::new(),
            detail_table: String::new(),
            detail_sys_id: String::new(),
            detail_field_cursor: 0,
            detail_loading: false,
            detail_history: Vec::new(),

            script_buf: String::new(),
            script_cursor: 0,
            script_output: Vec::new(),
            script_running: false,
            script_out_scroll: 0,

            column_cfg: crate::column_config::load(),
            display_names: true,
            current_list_name: None,
            current_list_static_columns: Vec::new(),
            col_picker_fields: Vec::new(),
            col_picker_cursor: 0,

            status: None,
            status_is_error: false,

            msg_tx,
            msg_rx,
        }
    }

    // ── Data loading ──────────────────────────────────────────────────────────

    pub async fn initial_health_check(&mut self) {
        self.spawn_health_check();
    }

    fn spawn_health_check(&self) {
        let client = self.client.clone();
        let tx = self.msg_tx.clone();
        tokio::spawn(async move {
            if let Ok(h) = client.health().await {
                let _ = tx.send(AppMsg::HealthUpdate(h));
            }
        });
    }

    fn load_records(&mut self, table: String, query: String, order: String) {
        let fields = self.effective_fields_param();
        let client = self.client.clone();
        let tx = self.msg_tx.clone();
        self.records_loading = true;
        self.records.clear();
        self.record_cursor = 0;
        self.record_scroll = 0;
        tokio::spawn(async move {
            match client.list_records(&table, &fields, &query, 100, &order).await {
                Ok(records) => {
                    let _ = tx.send(AppMsg::RecordsLoaded { table, records });
                }
                Err(e) => {
                    let _ = tx.send(AppMsg::Error(e));
                }
            }
        });
    }

    fn load_schema(&mut self, table: String) {
        let instance = self.health.instance_name.clone();
        // cache hit: send immediately without a network round-trip
        if let Some(meta) = self.schema_cache.get(&instance, &table) {
            let _ = self.msg_tx.send(AppMsg::SchemaLoaded { table, meta });
            return;
        }
        let client = self.client.clone();
        let cache = self.schema_cache.clone();
        let tx = self.msg_tx.clone();
        tokio::spawn(async move {
            match client.schema(&table).await {
                Ok(meta) => {
                    cache.set(&instance, &table, &meta);
                    let _ = tx.send(AppMsg::SchemaLoaded { table, meta });
                }
                Err(_) => {} // non-fatal: UI degrades gracefully without schema
            }
        });
    }

    // ── Effective columns ─────────────────────────────────────────────────────

    /// Resolves the columns to display for the current table/list view.
    /// Priority: user-saved list columns > static list columns > user-saved table columns
    ///           > built-in table def columns > hardcoded fallback.
    pub fn effective_columns(&self) -> Vec<ColumnDef> {
        let saved_fields: Option<&Vec<String>> =
            if let Some(ref name) = self.current_list_name {
                self.column_cfg.get_list(name)
                    .or_else(|| {
                        if !self.current_list_static_columns.is_empty() { None }
                        else { self.column_cfg.get_table(&self.current_table) }
                    })
            } else {
                self.column_cfg.get_table(&self.current_table)
            };

        let fields: Option<&Vec<String>> = saved_fields.or_else(|| {
            if self.current_list_static_columns.is_empty() {
                None
            } else {
                Some(&self.current_list_static_columns)
            }
        });

        if let Some(field_list) = fields {
            let def_map: std::collections::HashMap<&str, &ColumnDef> = self
                .current_table_def
                .as_ref()
                .map(|d| d.columns.iter().map(|c| (c.field.as_str(), c)).collect())
                .unwrap_or_default();
            return field_list
                .iter()
                .map(|f| {
                    def_map.get(f.as_str()).copied().cloned().unwrap_or_else(|| ColumnDef {
                        field: f.clone(),
                        header: f.clone(),
                        width: 0,
                    })
                })
                .collect();
        }

        if let Some(ref def) = self.current_table_def {
            return def.columns.clone();
        }

        vec![
            ColumnDef { field: "sys_id".into(), header: "sys_id".into(), width: 36 },
            ColumnDef { field: "sys_updated_on".into(), header: "Updated".into(), width: 0 },
        ]
    }

    /// Returns the comma-joined field list for the API request, always including sys_id.
    fn effective_fields_param(&self) -> String {
        let cols = self.effective_columns();
        let fields: Vec<&str> = cols.iter().map(|c| c.field.as_str()).collect();
        if fields.iter().any(|&f| f == "sys_id") {
            fields.join(",")
        } else {
            format!("sys_id,{}", fields.join(","))
        }
    }

    // ── Column picker ─────────────────────────────────────────────────────────

    pub fn open_col_picker(&mut self) {
        let active_fields: Vec<String> =
            self.effective_columns().into_iter().map(|c| c.field).collect();
        let active_set: std::collections::HashSet<&str> =
            active_fields.iter().map(|s| s.as_str()).collect();

        let mut entries: Vec<ColPickerEntry> = active_fields
            .iter()
            .map(|f| {
                let label = self
                    .current_schema
                    .as_ref()
                    .and_then(|s| s.columns.get(f))
                    .map(|c| c.label.clone())
                    .unwrap_or_default();
                ColPickerEntry { field: f.clone(), label, active: true }
            })
            .collect();

        if let Some(ref schema) = self.current_schema {
            let mut inactive: Vec<ColPickerEntry> = schema
                .columns
                .iter()
                .filter(|(f, _)| !active_set.contains(f.as_str()))
                .map(|(f, c)| ColPickerEntry {
                    field: f.clone(),
                    label: c.label.clone(),
                    active: false,
                })
                .collect();
            inactive.sort_by(|a, b| a.field.cmp(&b.field));
            entries.extend(inactive);
        }

        self.col_picker_fields = entries;
        self.col_picker_cursor = 0;
        self.overlay = Some(Overlay::ColumnPicker);
    }

    fn save_col_picker(&mut self) {
        let active: Vec<String> = self
            .col_picker_fields
            .iter()
            .filter(|e| e.active)
            .map(|e| e.field.clone())
            .collect();
        if active.is_empty() {
            return;
        }
        if let Some(ref name) = self.current_list_name.clone() {
            self.column_cfg.set_list(name, active);
        } else {
            self.column_cfg.set_table(&self.current_table.clone(), active);
        }
        crate::column_config::save(&self.column_cfg);
        let query = self.record_filter.clone();
        let order = self
            .current_table_def
            .as_ref()
            .map(|t| t.default_order.clone())
            .unwrap_or_default();
        let table = self.current_table.clone();
        self.load_records(table, query, order);
    }

    /// Re-sorts picker entries: active (in current order) first, then inactive (alpha).
    pub fn resort_col_picker(&mut self) {
        let (active, mut inactive): (Vec<_>, Vec<_>) =
            self.col_picker_fields.drain(..).partition(|e| e.active);
        inactive.sort_by(|a, b| a.field.cmp(&b.field));
        self.col_picker_fields = active;
        self.col_picker_fields.extend(inactive);
        let max = self.col_picker_fields.len().saturating_sub(1);
        self.col_picker_cursor = self.col_picker_cursor.min(max);
    }

    pub fn col_picker_move_up(&mut self) {
        let i = self.col_picker_cursor;
        if i > 0
            && self.col_picker_fields.get(i).map(|e| e.active).unwrap_or(false)
            && self.col_picker_fields.get(i - 1).map(|e| e.active).unwrap_or(false)
        {
            self.col_picker_fields.swap(i, i - 1);
            self.col_picker_cursor -= 1;
        }
    }

    pub fn col_picker_move_down(&mut self) {
        let i = self.col_picker_cursor;
        if i + 1 < self.col_picker_fields.len()
            && self.col_picker_fields.get(i).map(|e| e.active).unwrap_or(false)
            && self.col_picker_fields.get(i + 1).map(|e| e.active).unwrap_or(false)
        {
            self.col_picker_fields.swap(i, i + 1);
            self.col_picker_cursor += 1;
        }
    }

    fn load_detail(&mut self, table: String, sys_id: String) {
        let client = self.client.clone();
        let tx = self.msg_tx.clone();
        self.detail_loading = true;
        self.detail_record = None;
        self.detail_field_cursor = 0;
        tokio::spawn(async move {
            match client.get_record(&table, &sys_id, "all").await {
                Ok(record) => {
                    let _ = tx.send(AppMsg::RecordLoaded { record });
                }
                Err(e) => {
                    let _ = tx.send(AppMsg::Error(e));
                }
            }
        });
    }

    fn open_reference(&mut self, ref_table: String, ref_sys_id: String) {
        // push current position so Esc can return
        self.detail_history.push((self.detail_table.clone(), self.detail_sys_id.clone()));
        self.detail_table = ref_table.clone();
        self.detail_sys_id = ref_sys_id.clone();
        // load schema for the referenced table if not already cached
        self.load_schema(ref_table.clone());
        self.load_detail(ref_table, ref_sys_id);
    }

    fn spawn_run_script(&mut self) {
        let script = self.script_buf.trim().to_string();
        if script.is_empty() {
            return;
        }
        let client = self.client.clone();
        let tx = self.msg_tx.clone();
        self.script_running = true;
        self.script_output.push(">>> running...".into());
        self.script_out_scroll = self.script_output.len().saturating_sub(1);
        tokio::spawn(async move {
            let result = client.run_script(&script).await;
            let _ = tx.send(AppMsg::ScriptResult(result));
        });
    }

    fn load_all_tables(&mut self, filter: String) {
        let client = self.client.clone();
        let tx = self.msg_tx.clone();
        self.all_tables_loading = true;
        self.all_tables.clear();
        tokio::spawn(async move {
            match client.list_all_tables(&filter).await {
                Ok(tables) => {
                    let _ = tx.send(AppMsg::AllTablesLoaded(tables));
                }
                Err(e) => {
                    let _ = tx.send(AppMsg::Error(e));
                }
            }
        });
    }

    // ── Message processing ────────────────────────────────────────────────────

    pub fn process_messages(&mut self) {
        while let Ok(msg) = self.msg_rx.try_recv() {
            self.handle_msg(msg);
        }
    }

    fn handle_msg(&mut self, msg: AppMsg) {
        match msg {
            AppMsg::HealthUpdate(h) => {
                self.health = h;
            }
            AppMsg::RecordsLoaded { table, records } => {
                if table == self.current_table {
                    self.records = records;
                    self.records_loading = false;
                    self.record_cursor = 0;
                    self.record_scroll = 0;
                }
            }
            AppMsg::RecordLoaded { record } => {
                self.detail_field_keys = record
                    .as_object()
                    .map(|obj| tables::detail_field_order(obj))
                    .unwrap_or_default();
                self.detail_record = Some(record);
                self.detail_loading = false;
                self.detail_field_cursor = 0;
            }
            AppMsg::AllTablesLoaded(tables) => {
                self.all_tables = tables;
                self.all_tables_loading = false;
                self.all_tables_cursor = 0;
            }
            AppMsg::SchemaLoaded { table, meta } => {
                if table == self.current_table {
                    self.current_schema = Some(meta);
                }
            }
            AppMsg::ScriptResult(result) => {
                self.script_running = false;
                match result {
                    Ok(output) => {
                        for line in output.lines() {
                            self.script_output.push(line.to_string());
                        }
                        if output.trim().is_empty() {
                            self.script_output.push("(no output)".into());
                        }
                    }
                    Err(e) => {
                        self.script_output.push(format!("ERROR: {e}"));
                    }
                }
                self.script_out_scroll = self.script_output.len().saturating_sub(1);
            }
            AppMsg::Error(e) => {
                self.records_loading = false;
                self.detail_loading = false;
                self.all_tables_loading = false;
                self.status = Some(e);
                self.status_is_error = true;
            }
        }
    }

    // ── Periodic tick ─────────────────────────────────────────────────────────

    pub async fn tick(&mut self) {
        if self.last_health.elapsed() > Duration::from_secs(5) {
            self.last_health = Instant::now();
            self.spawn_health_check();
        }
    }

    // ── Navigation ────────────────────────────────────────────────────────────

    fn push_view(&mut self, view: BaseView) {
        let prev = std::mem::replace(&mut self.base_view, view);
        self.view_stack.push(prev);
    }

    fn pop_view(&mut self) {
        if let Some(prev) = self.view_stack.pop() {
            self.base_view = prev;
        }
    }

    fn open_table(&mut self, name: &str) {
        self.current_table = name.to_string();
        self.current_table_def = tables::find_table(&self.categories, name);
        self.current_list_name = None;
        self.current_list_static_columns = Vec::new();
        self.record_list_title = self
            .current_table_def
            .as_ref()
            .map(|t| t.label.clone())
            .unwrap_or_else(|| name.to_string());
        let query = self
            .current_table_def
            .as_ref()
            .map(|t| t.default_query.clone())
            .unwrap_or_default();
        let order = self
            .current_table_def
            .as_ref()
            .map(|t| t.default_order.clone())
            .unwrap_or_default();
        self.record_filter = query.clone();
        self.current_schema = None;
        self.push_view(BaseView::RecordList);
        self.load_records(name.to_string(), query, order);
        self.load_schema(name.to_string());
    }

    fn open_custom_list(
        &mut self,
        name: String,
        table: String,
        query: String,
        order: String,
        static_columns: Vec<String>,
    ) {
        self.current_table = table.clone();
        self.current_table_def = tables::find_table(&self.categories, &table);
        self.current_list_name = Some(name.clone());
        self.current_list_static_columns = static_columns;
        self.record_list_title = name;
        self.record_filter = query.clone();
        let effective_order = if order.is_empty() {
            self.current_table_def
                .as_ref()
                .map(|t| t.default_order.clone())
                .unwrap_or_default()
        } else {
            order
        };
        self.current_schema = None;
        self.push_view(BaseView::RecordList);
        self.load_records(table.clone(), query, effective_order);
        self.load_schema(table);
    }

    fn open_detail(&mut self) {
        if self.records.is_empty() {
            return;
        }
        let rec = &self.records[self.record_cursor];
        let sys_id = extract_sys_id(rec).unwrap_or_default();
        if sys_id.is_empty() {
            return;
        }
        let table = self.current_table.clone();
        self.detail_table = table.clone();
        self.detail_sys_id = sys_id.clone();
        self.push_view(BaseView::RecordDetail);
        self.load_detail(table, sys_id);
    }

    fn browser_down(&mut self) {
        let len = self.browser_items.len();
        let mut i = self.browser_cursor + 1;
        while i < len {
            if self.browser_items[i].is_selectable() {
                self.browser_cursor = i;
                return;
            }
            i += 1;
        }
    }

    fn browser_up(&mut self) {
        if self.browser_cursor == 0 {
            return;
        }
        let mut i = self.browser_cursor - 1;
        loop {
            if self.browser_items[i].is_selectable() {
                self.browser_cursor = i;
                return;
            }
            if i == 0 {
                return;
            }
            i -= 1;
        }
    }

    // ── Key handling ──────────────────────────────────────────────────────────

    /// Returns true if the app should quit.
    pub fn handle_key(&mut self, key: KeyEvent) -> bool {
        if key.modifiers == KeyModifiers::CONTROL && key.code == KeyCode::Char('c') {
            return true;
        }
        match self.mode {
            InputMode::Normal => self.handle_normal(key),
            InputMode::Filter => self.handle_filter_key(key),
            InputMode::Command => self.handle_command_key(key),
            InputMode::Script => self.handle_script_key(key),
        }
        self.should_quit
    }

    fn handle_normal(&mut self, key: KeyEvent) {
        if self.overlay.is_some() {
            self.handle_overlay_key(key);
            return;
        }
        match self.base_view.clone() {
            BaseView::TableBrowser => self.handle_browser(key),
            BaseView::AllTablesBrowser => self.handle_all_tables(key),
            BaseView::RecordList => self.handle_record_list(key),
            BaseView::RecordDetail => self.handle_detail(key),
        }
    }

    fn handle_overlay_key(&mut self, key: KeyEvent) {
        let ov = self.overlay.clone().unwrap();
        match ov {
            Overlay::Help => {
                self.overlay = None;
            }
            Overlay::ScriptRunner => match key.code {
                KeyCode::Esc => {
                    self.overlay = None;
                }
                KeyCode::Char('i') => {
                    self.mode = InputMode::Script;
                }
                KeyCode::Char('r') if key.modifiers == KeyModifiers::CONTROL => {
                    self.spawn_run_script();
                }
                KeyCode::Up | KeyCode::Char('k') => {
                    self.script_out_scroll = self.script_out_scroll.saturating_sub(1);
                }
                KeyCode::Down | KeyCode::Char('j') => {
                    let max = self.script_output.len().saturating_sub(1);
                    self.script_out_scroll = (self.script_out_scroll + 1).min(max);
                }
                _ => {}
            },
            Overlay::ColumnPicker => match key.code {
                KeyCode::Esc => {
                    self.save_col_picker();
                    self.overlay = None;
                }
                KeyCode::Up | KeyCode::Char('k') => {
                    self.col_picker_cursor = self.col_picker_cursor.saturating_sub(1);
                }
                KeyCode::Down | KeyCode::Char('j') => {
                    let max = self.col_picker_fields.len().saturating_sub(1);
                    self.col_picker_cursor = (self.col_picker_cursor + 1).min(max);
                }
                KeyCode::Char(' ') => {
                    if let Some(entry) = self.col_picker_fields.get_mut(self.col_picker_cursor) {
                        entry.active = !entry.active;
                    }
                    self.resort_col_picker();
                }
                KeyCode::Char('K') => self.col_picker_move_up(),
                KeyCode::Char('J') => self.col_picker_move_down(),
                KeyCode::Char('t') => {
                    self.display_names = !self.display_names;
                }
                _ => {}
            },
        }
    }

    fn handle_browser(&mut self, key: KeyEvent) {
        match key.code {
            KeyCode::Char('q') => {
                self.should_quit = true;
            }
            KeyCode::Char('?') => {
                self.overlay = Some(Overlay::Help);
            }
            KeyCode::Char('s') => {
                self.overlay = Some(Overlay::ScriptRunner);
            }
            KeyCode::Char(':') => {
                self.mode = InputMode::Command;
                self.command_buf.clear();
            }
            KeyCode::Down | KeyCode::Char('j') => self.browser_down(),
            KeyCode::Up | KeyCode::Char('k') => self.browser_up(),
            KeyCode::Char('g') => {
                self.browser_cursor = first_selectable(&self.browser_items, 0);
            }
            KeyCode::Char('G') => {
                let len = self.browser_items.len();
                if len > 0 {
                    self.browser_cursor = last_selectable(&self.browser_items);
                }
            }
            KeyCode::Enter => {
                let item = self.browser_items.get(self.browser_cursor).cloned();
                match item {
                    Some(BrowserItem::Table { name, .. }) => self.open_table(&name),
                    Some(BrowserItem::CustomList { name, table, query, order, columns }) => {
                        self.open_custom_list(name, table, query, order, columns);
                    }
                    Some(BrowserItem::BrowseAll) => {
                        self.push_view(BaseView::AllTablesBrowser);
                        self.load_all_tables(String::new());
                    }
                    _ => {}
                }
            }
            _ => {}
        }
    }

    fn handle_all_tables(&mut self, key: KeyEvent) {
        match key.code {
            KeyCode::Esc | KeyCode::Char('q') => self.pop_view(),
            KeyCode::Char('?') => {
                self.overlay = Some(Overlay::Help);
            }
            KeyCode::Char('s') => {
                self.overlay = Some(Overlay::ScriptRunner);
            }
            KeyCode::Char('/') => {
                self.mode = InputMode::Filter;
                self.filter_buf = self.all_tables_filter.clone();
            }
            KeyCode::Down | KeyCode::Char('j') => {
                let max = self.all_tables.len().saturating_sub(1);
                self.all_tables_cursor = (self.all_tables_cursor + 1).min(max);
            }
            KeyCode::Up | KeyCode::Char('k') => {
                self.all_tables_cursor = self.all_tables_cursor.saturating_sub(1);
            }
            KeyCode::Char('g') => self.all_tables_cursor = 0,
            KeyCode::Char('G') => {
                self.all_tables_cursor = self.all_tables.len().saturating_sub(1);
            }
            KeyCode::Enter => {
                if let Some((name, _)) = self.all_tables.get(self.all_tables_cursor).cloned() {
                    self.open_table(&name);
                }
            }
            _ => {}
        }
    }

    fn handle_record_list(&mut self, key: KeyEvent) {
        match key.code {
            KeyCode::Esc | KeyCode::Char('q') => self.pop_view(),
            KeyCode::Char('?') => {
                self.overlay = Some(Overlay::Help);
            }
            KeyCode::Char('s') => {
                self.overlay = Some(Overlay::ScriptRunner);
            }
            KeyCode::Char('c') => {
                self.open_col_picker();
            }
            KeyCode::Char('t') => {
                self.display_names = !self.display_names;
            }
            KeyCode::Char('r') => {
                let query = self.record_filter.clone();
                let order = self
                    .current_table_def
                    .as_ref()
                    .map(|t| t.default_order.clone())
                    .unwrap_or_default();
                let table = self.current_table.clone();
                self.load_records(table, query, order);
            }
            KeyCode::Char('R') => {
                if !self.records.is_empty() {
                    let sys_id = extract_sys_id(&self.records[self.record_cursor])
                        .unwrap_or_default();
                    let table = self.current_table.clone();
                    self.open_record_in_browser(&table, &sys_id);
                }
            }
            KeyCode::Char('/') => {
                self.mode = InputMode::Filter;
                self.filter_buf = self.record_filter.clone();
            }
            KeyCode::Char(':') => {
                self.mode = InputMode::Command;
                self.command_buf.clear();
            }
            KeyCode::Down | KeyCode::Char('j') => {
                let max = self.records.len().saturating_sub(1);
                self.record_cursor = (self.record_cursor + 1).min(max);
            }
            KeyCode::Up | KeyCode::Char('k') => {
                self.record_cursor = self.record_cursor.saturating_sub(1);
            }
            KeyCode::Char('g') => self.record_cursor = 0,
            KeyCode::Char('G') => {
                self.record_cursor = self.records.len().saturating_sub(1);
            }
            KeyCode::Enter | KeyCode::Char('d') => self.open_detail(),
            _ => {}
        }
    }

    fn handle_detail(&mut self, key: KeyEvent) {
        let n = self.detail_field_keys.len();
        match key.code {
            KeyCode::Esc | KeyCode::Char('q') => {
                if let Some((table, sys_id)) = self.detail_history.pop() {
                    // go back to previous record in the reference chain
                    self.detail_table = table.clone();
                    self.detail_sys_id = sys_id.clone();
                    self.load_schema(table.clone());
                    self.load_detail(table, sys_id);
                } else {
                    self.pop_view();
                }
            }
            KeyCode::Enter => {
                // follow a reference field if schema tells us it's a ref
                if let Some(field) = self.detail_field_keys.get(self.detail_field_cursor) {
                    let is_ref = self.current_schema
                        .as_ref()
                        .and_then(|s| s.columns.get(field))
                        .map(|c| c.is_reference() && !c.reference.is_empty())
                        .unwrap_or(false);
                    if is_ref {
                        let ref_table = self.current_schema
                            .as_ref()
                            .and_then(|s| s.columns.get(field))
                            .map(|c| c.reference.clone())
                            .unwrap_or_default();
                        // raw sys_id lives in record[field]["value"] (display_value=all response)
                        let ref_sys_id = self.detail_record
                            .as_ref()
                            .and_then(|r| r.get(field))
                            .and_then(|v| v.get("value").or(Some(v)))
                            .and_then(|v| v.as_str())
                            .unwrap_or_default()
                            .to_string();
                        if !ref_table.is_empty() && !ref_sys_id.is_empty() {
                            self.open_reference(ref_table, ref_sys_id);
                        }
                    }
                }
            }
            KeyCode::Char('?') => self.overlay = Some(Overlay::Help),
            KeyCode::Char('s') => self.overlay = Some(Overlay::ScriptRunner),
            KeyCode::Char('t') => self.display_names = !self.display_names,
            KeyCode::Char('r') => {
                let table = self.detail_table.clone();
                let sys_id = self.detail_sys_id.clone();
                self.load_detail(table, sys_id);
            }
            KeyCode::Char('R') => {
                let table = self.detail_table.clone();
                let sys_id = self.detail_sys_id.clone();
                self.open_record_in_browser(&table, &sys_id);
            }
            KeyCode::Char('c') => self.copy_current_field_value(),
            KeyCode::Down | KeyCode::Char('j') => {
                if n > 0 { self.detail_field_cursor = (self.detail_field_cursor + 1).min(n - 1); }
            }
            KeyCode::Up | KeyCode::Char('k') => {
                self.detail_field_cursor = self.detail_field_cursor.saturating_sub(1);
            }
            KeyCode::PageDown => {
                if n > 0 { self.detail_field_cursor = (self.detail_field_cursor + 20).min(n - 1); }
            }
            KeyCode::PageUp => {
                self.detail_field_cursor = self.detail_field_cursor.saturating_sub(20);
            }
            KeyCode::Char('g') => self.detail_field_cursor = 0,
            KeyCode::Char('G') => { if n > 0 { self.detail_field_cursor = n - 1; } }
            _ => {}
        }
    }

    fn copy_current_field_value(&mut self) {
        let field = match self.detail_field_keys.get(self.detail_field_cursor) {
            Some(f) => f.clone(),
            None => return,
        };
        let value = self
            .detail_record
            .as_ref()
            .and_then(|r| r.get(&field))
            .map(crate::tables::display_value)
            .unwrap_or_default();
        if copy_to_clipboard(&value) {
            let preview = truncate_chars(&value, 40);
            self.status = Some(format!("Copied: {preview}"));
            self.status_is_error = false;
        } else {
            self.status = Some("Clipboard unavailable (install wl-copy or xclip)".into());
            self.status_is_error = true;
        }
    }

    fn open_record_in_browser(&mut self, table: &str, sys_id: &str) {
        let instance_url = self.health.instance_url.trim_end_matches('/').to_string();
        if instance_url.is_empty() {
            self.status = Some("No instance connected".into());
            self.status_is_error = true;
            return;
        }
        let url = format!("{}/{}.do?sys_id={}", instance_url, table, sys_id);
        let _ = std::process::Command::new("xdg-open").arg(&url).spawn();
        self.status = Some(format!("Opening {url}"));
        self.status_is_error = false;
    }

    fn handle_filter_key(&mut self, key: KeyEvent) {
        match key.code {
            KeyCode::Esc => {
                self.mode = InputMode::Normal;
                self.filter_buf.clear();
            }
            KeyCode::Enter => {
                let query = self.filter_buf.clone();
                self.mode = InputMode::Normal;
                self.filter_buf.clear();
                match self.base_view {
                    BaseView::RecordList => {
                        self.record_filter = query.clone();
                        let order = self
                            .current_table_def
                            .as_ref()
                            .map(|t| t.default_order.clone())
                            .unwrap_or_default();
                        let table = self.current_table.clone();
                        self.load_records(table, query, order);
                    }
                    BaseView::AllTablesBrowser => {
                        self.all_tables_filter = query.clone();
                        self.load_all_tables(query);
                    }
                    _ => {}
                }
            }
            KeyCode::Backspace => {
                self.filter_buf.pop();
            }
            KeyCode::Char(c) => {
                self.filter_buf.push(c);
            }
            _ => {}
        }
    }

    fn handle_command_key(&mut self, key: KeyEvent) {
        match key.code {
            KeyCode::Esc => {
                self.mode = InputMode::Normal;
                self.command_buf.clear();
            }
            KeyCode::Enter => {
                let cmd = self.command_buf.trim().to_string();
                self.mode = InputMode::Normal;
                self.command_buf.clear();
                self.execute_command(&cmd);
            }
            KeyCode::Backspace => {
                self.command_buf.pop();
            }
            KeyCode::Char(c) => {
                self.command_buf.push(c);
            }
            _ => {}
        }
    }

    fn execute_command(&mut self, cmd: &str) {
        match cmd.trim() {
            "" => {}
            "q" | "quit" => self.should_quit = true,
            table => self.open_table(table),
        }
    }

    fn handle_script_key(&mut self, key: KeyEvent) {
        match key.code {
            KeyCode::Esc => {
                self.mode = InputMode::Normal;
            }
            KeyCode::Enter if key.modifiers == KeyModifiers::CONTROL => {
                self.mode = InputMode::Normal;
                self.spawn_run_script();
            }
            KeyCode::Enter => {
                let pos = self.script_cursor;
                self.script_buf.insert(pos, '\n');
                self.script_cursor = pos + 1;
            }
            KeyCode::Backspace => {
                if self.script_cursor > 0 {
                    let pos = self.script_cursor - 1;
                    self.script_buf.remove(pos);
                    self.script_cursor = pos;
                }
            }
            KeyCode::Char('r') if key.modifiers == KeyModifiers::CONTROL => {
                self.mode = InputMode::Normal;
                self.spawn_run_script();
            }
            KeyCode::F(5) => {
                self.mode = InputMode::Normal;
                self.spawn_run_script();
            }
            KeyCode::Char(c) => {
                let pos = self.script_cursor;
                self.script_buf.insert(pos, c);
                self.script_cursor = pos + 1;
            }
            KeyCode::Left => {
                self.script_cursor = self.script_cursor.saturating_sub(1);
            }
            KeyCode::Right => {
                self.script_cursor = (self.script_cursor + 1).min(self.script_buf.len());
            }
            _ => {}
        }
    }
}

// ── Helpers ───────────────────────────────────────────────────────────────────

fn build_browser_items(categories: &[Category], custom_lists: &[CustomList]) -> Vec<BrowserItem> {
    let mut items = Vec::new();

    if !custom_lists.is_empty() {
        items.push(BrowserItem::Header("MY LISTS".to_string()));
        for list in custom_lists {
            items.push(BrowserItem::CustomList {
                name: list.name.clone(),
                table: list.table.clone(),
                query: list.query.clone(),
                order: list.order.clone(),
                columns: list.columns.clone(),
            });
        }
        items.push(BrowserItem::Header(String::new())); // separator
    }

    for cat in categories {
        items.push(BrowserItem::Header(cat.name.to_string()));
        for t in &cat.tables {
            items.push(BrowserItem::Table { name: t.name.clone(), label: t.label.clone() });
        }
    }
    items.push(BrowserItem::Header(String::new()));
    items.push(BrowserItem::BrowseAll);
    items
}

fn first_selectable(items: &[BrowserItem], from: usize) -> usize {
    for (i, item) in items.iter().enumerate().skip(from) {
        if item.is_selectable() {
            return i;
        }
    }
    from
}

fn last_selectable(items: &[BrowserItem]) -> usize {
    for (i, item) in items.iter().enumerate().rev() {
        if item.is_selectable() {
            return i;
        }
    }
    0
}

fn copy_to_clipboard(text: &str) -> bool {
    use std::io::Write;
    use std::process::{Command, Stdio};

    // Try wl-copy (Wayland)
    if let Ok(mut child) = Command::new("wl-copy").stdin(Stdio::piped()).spawn() {
        if let Some(mut stdin) = child.stdin.take() {
            let _ = stdin.write_all(text.as_bytes());
        }
        return true;
    }
    // Fall back to xclip (X11)
    if let Ok(mut child) = Command::new("xclip")
        .args(["-selection", "clipboard"])
        .stdin(Stdio::piped())
        .spawn()
    {
        if let Some(mut stdin) = child.stdin.take() {
            let _ = stdin.write_all(text.as_bytes());
        }
        return true;
    }
    false
}

fn truncate_chars(s: &str, max: usize) -> String {
    let mut chars = s.chars();
    let head: String = chars.by_ref().take(max).collect();
    if chars.next().is_some() { format!("{head}…") } else { head }
}

fn extract_sys_id(record: &Value) -> Option<String> {
    let v = record.get("sys_id")?;
    match v {
        Value::String(s) => Some(s.clone()),
        Value::Object(m) => m.get("value").and_then(|v| v.as_str()).map(|s| s.to_string()),
        _ => None,
    }
}
