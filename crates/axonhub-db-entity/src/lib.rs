pub mod api_keys {
    use sea_orm::entity::prelude::*;

    #[derive(Clone, Debug, PartialEq, DeriveEntityModel)]
    #[sea_orm(table_name = "api_keys")]
    pub struct Model {
        #[sea_orm(primary_key)]
        pub id: i64,
        pub user_id: i64,
        pub project_id: i64,
        pub key: String,
        pub name: String,
        #[sea_orm(column_name = "type")]
        pub type_field: String,
        pub status: String,
        pub scopes: String,
        pub profiles: String,
        pub deleted_at: i64,
    }

    #[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
    pub enum Relation {}

    impl ActiveModelBehavior for ActiveModel {}
}

pub mod channel_probes {
    use sea_orm::entity::prelude::*;

    #[derive(Clone, Debug, PartialEq, DeriveEntityModel)]
    #[sea_orm(table_name = "channel_probes")]
    pub struct Model {
        #[sea_orm(primary_key)]
        pub id: i64,
        pub channel_id: i64,
        pub timestamp: i64,
        pub total_request_count: i32,
        pub success_request_count: i32,
        pub avg_tokens_per_second: Option<f64>,
        pub avg_time_to_first_token_ms: Option<f64>,
    }

    #[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
    pub enum Relation {}

    impl ActiveModelBehavior for ActiveModel {}
}

pub mod channels {
    use sea_orm::entity::prelude::*;

    #[derive(Clone, Debug, PartialEq, DeriveEntityModel)]
    #[sea_orm(table_name = "channels")]
    pub struct Model {
        #[sea_orm(primary_key)]
        pub id: i64,
        #[sea_orm(column_name = "type")]
        pub type_field: String,
        pub base_url: String,
        pub name: String,
        pub status: String,
        pub credentials: String,
        pub supported_models: String,
        pub auto_sync_supported_models: bool,
        pub default_test_model: String,
        pub settings: String,
        pub tags: String,
        pub ordering_weight: i64,
        pub error_message: String,
        pub remark: String,
        pub deleted_at: i64,
    }

    #[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
    pub enum Relation {}

    impl ActiveModelBehavior for ActiveModel {}
}

pub mod data_storages {
    use sea_orm::entity::prelude::*;

    #[derive(Clone, Debug, PartialEq, DeriveEntityModel)]
    #[sea_orm(table_name = "data_storages")]
    pub struct Model {
        #[sea_orm(primary_key)]
        pub id: i64,
        pub name: String,
        pub description: String,
        #[sea_orm(column_name = "primary")]
        pub primary_flag: bool,
        #[sea_orm(column_name = "type")]
        pub type_field: String,
        pub settings: String,
        pub status: String,
        pub deleted_at: i64,
    }

    #[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
    pub enum Relation {}

    impl ActiveModelBehavior for ActiveModel {}
}

pub mod models {
    use sea_orm::entity::prelude::*;

    #[derive(Clone, Debug, PartialEq, DeriveEntityModel)]
    #[sea_orm(table_name = "models")]
    pub struct Model {
        #[sea_orm(primary_key)]
        pub id: i64,
        pub developer: String,
        pub model_id: String,
        #[sea_orm(column_name = "type")]
        pub type_field: String,
        pub name: String,
        pub icon: String,
        #[sea_orm(column_name = "group")]
        pub group_name: String,
        pub model_card: String,
        pub settings: String,
        pub status: String,
        pub remark: String,
        pub deleted_at: i64,
    }

    #[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
    pub enum Relation {}

    impl ActiveModelBehavior for ActiveModel {}
}

pub mod projects {
    use sea_orm::entity::prelude::*;

    #[derive(Clone, Debug, PartialEq, DeriveEntityModel)]
    #[sea_orm(table_name = "projects")]
    pub struct Model {
        #[sea_orm(primary_key)]
        pub id: i64,
        pub name: String,
        pub description: String,
        pub status: String,
        pub deleted_at: i64,
    }

    #[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
    pub enum Relation {}

    impl ActiveModelBehavior for ActiveModel {}
}

pub mod provider_quota_statuses {
    use sea_orm::entity::prelude::*;

    #[derive(Clone, Debug, PartialEq, DeriveEntityModel)]
    #[sea_orm(table_name = "provider_quota_statuses")]
    pub struct Model {
        #[sea_orm(primary_key)]
        pub id: i64,
        pub channel_id: i64,
        pub provider_type: String,
        pub status: String,
        pub quota_data: String,
        pub next_reset_at: Option<i64>,
        pub ready: bool,
        pub next_check_at: i64,
    }

    #[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
    pub enum Relation {}

    impl ActiveModelBehavior for ActiveModel {}
}

pub mod request_executions {
    use sea_orm::entity::prelude::*;

    #[derive(Clone, Debug, PartialEq, DeriveEntityModel)]
    #[sea_orm(table_name = "request_executions")]
    pub struct Model {
        #[sea_orm(primary_key)]
        pub id: i64,
        pub project_id: i64,
        pub request_id: i64,
        pub channel_id: Option<i64>,
        pub data_storage_id: Option<i64>,
        pub external_id: Option<String>,
        pub model_id: String,
        pub format: String,
        pub request_body: String,
        pub response_body: Option<String>,
        pub response_chunks: Option<String>,
        pub error_message: String,
        pub response_status_code: Option<i64>,
        pub status: String,
        pub stream: bool,
        pub metrics_latency_ms: Option<i64>,
        pub metrics_first_token_latency_ms: Option<i64>,
        pub request_headers: String,
    }

    #[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
    pub enum Relation {}

    impl ActiveModelBehavior for ActiveModel {}
}

pub mod requests {
    use sea_orm::entity::prelude::*;

    #[derive(Clone, Debug, PartialEq, DeriveEntityModel)]
    #[sea_orm(table_name = "requests")]
    pub struct Model {
        #[sea_orm(primary_key)]
        pub id: i64,
        pub api_key_id: Option<i64>,
        pub project_id: i64,
        pub trace_id: Option<i64>,
        pub data_storage_id: Option<i64>,
        pub source: String,
        pub model_id: String,
        pub format: String,
        pub request_headers: String,
        pub request_body: String,
        pub response_body: Option<String>,
        pub response_chunks: Option<String>,
        pub channel_id: Option<i64>,
        pub external_id: Option<String>,
        pub status: String,
        pub stream: bool,
        pub client_ip: String,
        pub metrics_latency_ms: Option<i64>,
        pub metrics_first_token_latency_ms: Option<i64>,
        pub content_saved: bool,
        pub content_storage_id: Option<i64>,
        pub content_storage_key: Option<String>,
        pub content_saved_at: Option<String>,
    }

    #[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
    pub enum Relation {}

    impl ActiveModelBehavior for ActiveModel {}
}

pub mod roles {
    use sea_orm::entity::prelude::*;

    #[derive(Clone, Debug, PartialEq, DeriveEntityModel)]
    #[sea_orm(table_name = "roles")]
    pub struct Model {
        #[sea_orm(primary_key)]
        pub id: i64,
        pub name: String,
        pub level: String,
        pub project_id: i64,
        pub scopes: String,
        pub deleted_at: i64,
    }

    #[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
    pub enum Relation {}

    impl ActiveModelBehavior for ActiveModel {}
}

pub mod systems {
    use sea_orm::entity::prelude::*;

    #[derive(Clone, Debug, PartialEq, DeriveEntityModel)]
    #[sea_orm(table_name = "systems")]
    pub struct Model {
        #[sea_orm(primary_key)]
        pub id: i64,
        pub key: String,
        pub value: String,
        pub deleted_at: i64,
    }

    #[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
    pub enum Relation {}

    impl ActiveModelBehavior for ActiveModel {}
}

pub mod threads {
    use sea_orm::entity::prelude::*;

    #[derive(Clone, Debug, PartialEq, DeriveEntityModel)]
    #[sea_orm(table_name = "threads")]
    pub struct Model {
        #[sea_orm(primary_key)]
        pub id: i64,
        pub project_id: i64,
        pub thread_id: String,
    }

    #[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
    pub enum Relation {}

    impl ActiveModelBehavior for ActiveModel {}
}

pub mod traces {
    use sea_orm::entity::prelude::*;

    #[derive(Clone, Debug, PartialEq, DeriveEntityModel)]
    #[sea_orm(table_name = "traces")]
    pub struct Model {
        #[sea_orm(primary_key)]
        pub id: i64,
        pub project_id: i64,
        pub trace_id: String,
        pub thread_id: Option<i64>,
    }

    #[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
    pub enum Relation {}

    impl ActiveModelBehavior for ActiveModel {}
}

pub mod usage_logs {
    use sea_orm::entity::prelude::*;

    #[derive(Clone, Debug, PartialEq, DeriveEntityModel)]
    #[sea_orm(table_name = "usage_logs")]
    pub struct Model {
        #[sea_orm(primary_key)]
        pub id: i64,
        pub request_id: i64,
        pub api_key_id: Option<i64>,
        pub project_id: i64,
        pub channel_id: Option<i64>,
        pub model_id: String,
        pub prompt_tokens: i64,
        pub completion_tokens: i64,
        pub total_tokens: i64,
        pub prompt_audio_tokens: i64,
        pub prompt_cached_tokens: i64,
        pub prompt_write_cached_tokens: i64,
        pub prompt_write_cached_tokens_5m: i64,
        pub prompt_write_cached_tokens_1h: i64,
        pub completion_audio_tokens: i64,
        pub completion_reasoning_tokens: i64,
        pub completion_accepted_prediction_tokens: i64,
        pub completion_rejected_prediction_tokens: i64,
        pub source: String,
        pub format: String,
        pub total_cost: Option<f64>,
        pub cost_items: String,
        pub cost_price_reference_id: String,
        pub deleted_at: i64,
    }

    #[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
    pub enum Relation {}

    impl ActiveModelBehavior for ActiveModel {}
}

pub mod user_projects {
    use sea_orm::entity::prelude::*;

    #[derive(Clone, Debug, PartialEq, DeriveEntityModel)]
    #[sea_orm(table_name = "user_projects")]
    pub struct Model {
        #[sea_orm(primary_key)]
        pub id: i64,
        pub user_id: i64,
        pub project_id: i64,
        pub is_owner: bool,
        pub scopes: String,
    }

    #[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
    pub enum Relation {}

    impl ActiveModelBehavior for ActiveModel {}
}

pub mod user_roles {
    use sea_orm::entity::prelude::*;

    #[derive(Clone, Debug, PartialEq, DeriveEntityModel)]
    #[sea_orm(table_name = "user_roles")]
    pub struct Model {
        #[sea_orm(primary_key)]
        pub id: i64,
        pub user_id: i64,
        pub role_id: i64,
    }

    #[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
    pub enum Relation {}

    impl ActiveModelBehavior for ActiveModel {}
}

pub mod users {
    use sea_orm::entity::prelude::*;

    #[derive(Clone, Debug, PartialEq, DeriveEntityModel)]
    #[sea_orm(table_name = "users")]
    pub struct Model {
        #[sea_orm(primary_key)]
        pub id: i64,
        pub email: String,
        pub status: String,
        pub prefer_language: String,
        pub password: String,
        pub first_name: String,
        pub last_name: String,
        pub avatar: String,
        pub is_owner: bool,
        pub scopes: String,
        pub deleted_at: i64,
    }

    #[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
    pub enum Relation {}

    impl ActiveModelBehavior for ActiveModel {}
}
