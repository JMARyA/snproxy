use serde_json::Value;

#[derive(Debug, Clone)]
pub struct ColumnDef {
    pub field: String,
    pub header: String,
    pub width: u16, // 0 = fill remaining space
}

#[derive(Debug, Clone)]
pub struct TableDef {
    pub name: String,
    pub label: String,
    pub fields: String,
    pub columns: Vec<ColumnDef>,
    pub default_query: String,
    pub default_order: String,
}

impl TableDef {
    fn new(
        name: &str,
        label: &str,
        fields: &str,
        cols: &[(&str, &str, u16)],
        query: &str,
        order: &str,
    ) -> Self {
        Self {
            name: name.into(),
            label: label.into(),
            fields: fields.into(),
            columns: cols
                .iter()
                .map(|&(f, h, w)| ColumnDef { field: f.into(), header: h.into(), width: w })
                .collect(),
            default_query: query.into(),
            default_order: order.into(),
        }
    }
}

pub struct Category {
    pub name: &'static str,
    pub tables: Vec<TableDef>,
}

pub fn make_categories() -> Vec<Category> {
    vec![
        Category {
            name: "ITSM",
            tables: vec![
                TableDef::new(
                    "incident",
                    "Incidents",
                    "number,short_description,state,priority,assigned_to,sys_updated_on",
                    &[
                        ("number", "Number", 12),
                        ("short_description", "Short Description", 0),
                        ("state", "State", 12),
                        ("priority", "P", 3),
                        ("assigned_to", "Assigned To", 20),
                        ("sys_updated_on", "Updated", 16),
                    ],
                    "active=true",
                    "ORDERBYDESCsys_updated_on",
                ),
                TableDef::new(
                    "change_request",
                    "Changes",
                    "number,short_description,state,risk,assigned_to,start_date",
                    &[
                        ("number", "Number", 12),
                        ("short_description", "Short Description", 0),
                        ("state", "State", 12),
                        ("risk", "Risk", 8),
                        ("assigned_to", "Assigned To", 20),
                        ("start_date", "Start Date", 16),
                    ],
                    "",
                    "ORDERBYDESCsys_updated_on",
                ),
                TableDef::new(
                    "problem",
                    "Problems",
                    "number,short_description,state,priority,assigned_to,sys_updated_on",
                    &[
                        ("number", "Number", 12),
                        ("short_description", "Short Description", 0),
                        ("state", "State", 12),
                        ("priority", "P", 3),
                        ("assigned_to", "Assigned To", 20),
                        ("sys_updated_on", "Updated", 16),
                    ],
                    "active=true",
                    "ORDERBYDESCsys_updated_on",
                ),
            ],
        },
        Category {
            name: "SERVICE CATALOG",
            tables: vec![
                TableDef::new(
                    "sc_request",
                    "Requests",
                    "number,short_description,state,opened_by,sys_updated_on",
                    &[
                        ("number", "Number", 12),
                        ("short_description", "Short Description", 0),
                        ("state", "State", 12),
                        ("opened_by", "Opened By", 20),
                        ("sys_updated_on", "Updated", 16),
                    ],
                    "active=true",
                    "ORDERBYDESCsys_updated_on",
                ),
                TableDef::new(
                    "sc_task",
                    "Request Tasks",
                    "number,short_description,state,assigned_to,sys_updated_on",
                    &[
                        ("number", "Number", 12),
                        ("short_description", "Short Description", 0),
                        ("state", "State", 12),
                        ("assigned_to", "Assigned To", 20),
                        ("sys_updated_on", "Updated", 16),
                    ],
                    "active=true",
                    "ORDERBYDESCsys_updated_on",
                ),
            ],
        },
        Category {
            name: "TASKS",
            tables: vec![
                TableDef::new(
                    "task",
                    "Tasks",
                    "number,short_description,state,priority,assigned_to,sys_updated_on",
                    &[
                        ("number", "Number", 12),
                        ("short_description", "Short Description", 0),
                        ("state", "State", 12),
                        ("priority", "P", 3),
                        ("assigned_to", "Assigned To", 20),
                        ("sys_updated_on", "Updated", 16),
                    ],
                    "active=true",
                    "ORDERBYDESCsys_updated_on",
                ),
                TableDef::new(
                    "sysapproval_approver",
                    "Approvals",
                    "state,approver,source_table,sys_updated_on",
                    &[
                        ("state", "State", 12),
                        ("approver", "Approver", 0),
                        ("source_table", "Table", 20),
                        ("sys_updated_on", "Updated", 16),
                    ],
                    "state=requested",
                    "ORDERBYDESCsys_updated_on",
                ),
            ],
        },
        Category {
            name: "USERS & GROUPS",
            tables: vec![
                TableDef::new(
                    "sys_user",
                    "Users",
                    "user_name,name,email,active,sys_updated_on",
                    &[
                        ("user_name", "Username", 16),
                        ("name", "Full Name", 0),
                        ("email", "Email", 30),
                        ("active", "Active", 8),
                        ("sys_updated_on", "Updated", 16),
                    ],
                    "active=true",
                    "ORDERBYname",
                ),
                TableDef::new(
                    "sys_group",
                    "Groups",
                    "name,description,active,sys_updated_on",
                    &[
                        ("name", "Name", 30),
                        ("description", "Description", 0),
                        ("active", "Active", 8),
                        ("sys_updated_on", "Updated", 16),
                    ],
                    "active=true",
                    "ORDERBYname",
                ),
            ],
        },
        Category {
            name: "CMDB",
            tables: vec![TableDef::new(
                "cmdb_ci",
                "Configuration Items",
                "name,sys_class_name,operational_status,assigned_to,sys_updated_on",
                &[
                    ("name", "Name", 0),
                    ("sys_class_name", "Class", 25),
                    ("operational_status", "Status", 12),
                    ("assigned_to", "Assigned To", 20),
                    ("sys_updated_on", "Updated", 16),
                ],
                "",
                "ORDERBYname",
            )],
        },
    ]
}

pub fn find_table(categories: &[Category], name: &str) -> Option<TableDef> {
    for cat in categories {
        for t in &cat.tables {
            if t.name == name {
                return Some(t.clone());
            }
        }
    }
    None
}

pub fn display_value(v: &Value) -> String {
    match v {
        Value::String(s) => s.clone(),
        Value::Null => String::new(),
        Value::Bool(b) => b.to_string(),
        Value::Number(n) => n.to_string(),
        Value::Object(m) => m
            .get("display_value")
            .or_else(|| m.get("value"))
            .and_then(|v| v.as_str())
            .unwrap_or_default()
            .to_string(),
        Value::Array(_) => "[array]".into(),
    }
}
