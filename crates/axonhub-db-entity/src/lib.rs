pub mod api_keys {
    use sea_orm::{entity::prelude::*, FromQueryResult};

    #[derive(Clone, Debug, PartialEq, DeriveEntityModel)]
    #[sea_orm(table_name = "api_keys")]
    pub struct Model {
        #[sea_orm(primary_key)]
        pub id: i64,
        pub created_at: String,
        pub updated_at: String,
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
    pub enum Relation {
        #[sea_orm(
            belongs_to = "super::users::Entity",
            from = "Column::UserId",
            to = "super::users::Column::Id"
        )]
        Users,
        #[sea_orm(
            belongs_to = "super::projects::Entity",
            from = "Column::ProjectId",
            to = "super::projects::Column::Id"
        )]
        Projects,
        #[sea_orm(has_many = "super::requests::Entity")]
        Requests,
        #[sea_orm(has_many = "super::usage_logs::Entity")]
        UsageLogs,
    }

    impl Related<super::users::Entity> for Entity {
        fn to() -> RelationDef {
            Relation::Users.def()
        }
    }

    impl Related<super::projects::Entity> for Entity {
        fn to() -> RelationDef {
            Relation::Projects.def()
        }
    }

    impl Related<super::requests::Entity> for Entity {
        fn to() -> RelationDef {
            Relation::Requests.def()
        }
    }

    impl Related<super::usage_logs::Entity> for Entity {
        fn to() -> RelationDef {
            Relation::UsageLogs.def()
        }
    }

    #[derive(Clone, Debug, PartialEq, Eq, DerivePartialModel, FromQueryResult)]
    #[sea_orm(entity = "Entity")]
    pub struct AuthLookup {
        pub id: i64,
        pub user_id: i64,
        pub key: String,
        pub name: String,
        #[sea_orm(from_col = "type_field")]
        pub key_type: String,
        pub status: String,
        pub project_id: i64,
        pub scopes: String,
    }

    #[derive(Clone, Debug, PartialEq, Eq, DerivePartialModel, FromQueryResult)]
    #[sea_orm(entity = "Entity")]
    pub struct OwnerLookup {
        pub user_id: i64,
        #[sea_orm(from_col = "type_field")]
        pub key_type: String,
        pub project_id: i64,
    }

    #[derive(Clone, Debug, PartialEq, Eq, DerivePartialModel, FromQueryResult)]
    #[sea_orm(entity = "Entity")]
    pub struct ProfilesOnly {
        pub profiles: String,
    }

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
    pub enum Relation {
        #[sea_orm(
            belongs_to = "super::channels::Entity",
            from = "Column::ChannelId",
            to = "super::channels::Column::Id"
        )]
        Channels,
    }

    impl Related<super::channels::Entity> for Entity {
        fn to() -> RelationDef {
            Relation::Channels.def()
        }
    }

    impl ActiveModelBehavior for ActiveModel {}
}

pub mod channel_model_price_versions {
    use sea_orm::entity::prelude::*;

    #[derive(Clone, Debug, PartialEq, DeriveEntityModel)]
    #[sea_orm(table_name = "channel_model_price_versions")]
    pub struct Model {
        #[sea_orm(primary_key)]
        pub id: i64,
        pub created_at: String,
        pub updated_at: String,
        pub channel_id: i64,
        pub model_id: String,
        pub channel_model_price_id: i64,
        pub price: String,
        pub status: String,
        pub effective_start_at: String,
        pub effective_end_at: Option<String>,
        pub reference_id: String,
    }

    #[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
    pub enum Relation {
        #[sea_orm(
            belongs_to = "super::channel_model_prices::Entity",
            from = "Column::ChannelModelPriceId",
            to = "super::channel_model_prices::Column::Id"
        )]
        ChannelModelPrices,
    }

    impl Related<super::channel_model_prices::Entity> for Entity {
        fn to() -> RelationDef {
            Relation::ChannelModelPrices.def()
        }
    }

    impl ActiveModelBehavior for ActiveModel {}
}

pub mod channel_model_prices {
    use sea_orm::entity::prelude::*;

    #[derive(Clone, Debug, PartialEq, DeriveEntityModel)]
    #[sea_orm(table_name = "channel_model_prices")]
    pub struct Model {
        #[sea_orm(primary_key)]
        pub id: i64,
        pub created_at: String,
        pub updated_at: String,
        pub channel_id: i64,
        pub model_id: String,
        pub price: String,
        pub reference_id: String,
        pub deleted_at: i64,
    }

    #[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
    pub enum Relation {
        #[sea_orm(
            belongs_to = "super::channels::Entity",
            from = "Column::ChannelId",
            to = "super::channels::Column::Id"
        )]
        Channels,
        #[sea_orm(has_many = "super::channel_model_price_versions::Entity")]
        ChannelModelPriceVersions,
    }

    impl Related<super::channels::Entity> for Entity {
        fn to() -> RelationDef {
            Relation::Channels.def()
        }
    }

    impl Related<super::channel_model_price_versions::Entity> for Entity {
        fn to() -> RelationDef {
            Relation::ChannelModelPriceVersions.def()
        }
    }

    impl ActiveModelBehavior for ActiveModel {}
}

pub mod channels {
    use sea_orm::{entity::prelude::*, FromQueryResult};

    #[derive(Clone, Debug, PartialEq, DeriveEntityModel)]
    #[sea_orm(table_name = "channels")]
    pub struct Model {
        #[sea_orm(primary_key)]
        pub id: i64,
        pub created_at: String,
        pub updated_at: String,
        #[sea_orm(column_name = "type")]
        pub type_field: String,
        pub base_url: Option<String>,
        pub name: String,
        pub status: String,
        pub credentials: String,
        pub disabled_api_keys: String,
        pub supported_models: String,
        pub manual_models: String,
        pub auto_sync_supported_models: bool,
        pub auto_sync_model_pattern: String,
        pub tags: String,
        pub default_test_model: String,
        pub policies: String,
        pub settings: String,
        pub ordering_weight: i32,
        pub error_message: Option<String>,
        pub remark: Option<String>,
        pub deleted_at: i64,
    }

    #[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
    pub enum Relation {
        #[sea_orm(has_many = "super::channel_probes::Entity")]
        ChannelProbes,
        #[sea_orm(has_many = "super::channel_model_prices::Entity")]
        ChannelModelPrices,
        #[sea_orm(has_many = "super::provider_quota_statuses::Entity")]
        ProviderQuotaStatuses,
        #[sea_orm(has_many = "super::request_executions::Entity")]
        RequestExecutions,
        #[sea_orm(has_many = "super::requests::Entity")]
        Requests,
        #[sea_orm(has_many = "super::usage_logs::Entity")]
        UsageLogs,
    }

    impl Related<super::channel_probes::Entity> for Entity {
        fn to() -> RelationDef {
            Relation::ChannelProbes.def()
        }
    }

    impl Related<super::channel_model_prices::Entity> for Entity {
        fn to() -> RelationDef {
            Relation::ChannelModelPrices.def()
        }
    }

    impl Related<super::provider_quota_statuses::Entity> for Entity {
        fn to() -> RelationDef {
            Relation::ProviderQuotaStatuses.def()
        }
    }

    impl Related<super::request_executions::Entity> for Entity {
        fn to() -> RelationDef {
            Relation::RequestExecutions.def()
        }
    }

    impl Related<super::requests::Entity> for Entity {
        fn to() -> RelationDef {
            Relation::Requests.def()
        }
    }

    impl Related<super::usage_logs::Entity> for Entity {
        fn to() -> RelationDef {
            Relation::UsageLogs.def()
        }
    }

    #[derive(Clone, Debug, PartialEq, Eq, DerivePartialModel, FromQueryResult)]
    #[sea_orm(entity = "Entity")]
    pub struct RoutingCandidate {
        pub id: i64,
        pub base_url: Option<String>,
        pub credentials: String,
        pub supported_models: String,
        pub settings: String,
        pub ordering_weight: i32,
        #[sea_orm(from_col = "type_field")]
        pub channel_type: String,
        pub status: String,
    }

    impl ActiveModelBehavior for ActiveModel {}
}

pub mod channel_override_templates {
    use sea_orm::entity::prelude::*;

    #[derive(Clone, Debug, PartialEq, DeriveEntityModel)]
    #[sea_orm(table_name = "channel_override_templates")]
    pub struct Model {
        #[sea_orm(primary_key)]
        pub id: i64,
        pub created_at: String,
        pub updated_at: String,
        pub user_id: i64,
        pub name: String,
        pub description: Option<String>,
        pub override_parameters: String,
        pub override_headers: String,
        pub header_override_operations: String,
        pub body_override_operations: String,
        pub deleted_at: i64,
    }

    #[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
    pub enum Relation {
        #[sea_orm(
            belongs_to = "super::users::Entity",
            from = "Column::UserId",
            to = "super::users::Column::Id"
        )]
        Users,
    }

    impl Related<super::users::Entity> for Entity {
        fn to() -> RelationDef {
            Relation::Users.def()
        }
    }

    impl ActiveModelBehavior for ActiveModel {}
}

pub mod data_storages {
    use sea_orm::{entity::prelude::*, FromQueryResult};

    #[derive(Clone, Debug, PartialEq, DeriveEntityModel)]
    #[sea_orm(table_name = "data_storages")]
    pub struct Model {
        #[sea_orm(primary_key)]
        pub id: i64,
        pub created_at: String,
        pub updated_at: String,
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
    pub enum Relation {
        #[sea_orm(has_many = "super::request_executions::Entity")]
        RequestExecutions,
        #[sea_orm(has_many = "super::requests::Entity")]
        Requests,
    }

    impl Related<super::request_executions::Entity> for Entity {
        fn to() -> RelationDef {
            Relation::RequestExecutions.def()
        }
    }

    impl Related<super::requests::Entity> for Entity {
        fn to() -> RelationDef {
            Relation::Requests.def()
        }
    }

    #[derive(Clone, Debug, PartialEq, Eq, DerivePartialModel, FromQueryResult)]
    #[sea_orm(entity = "Entity")]
    pub struct StorageConfig {
        pub id: i64,
        #[sea_orm(from_col = "type_field")]
        pub storage_type: String,
        pub settings: String,
        pub status: String,
    }

    #[derive(Clone, Debug, PartialEq, Eq, DerivePartialModel, FromQueryResult)]
    #[sea_orm(entity = "Entity")]
    pub struct GraphqlStatus {
        pub id: i64,
        pub status: String,
    }

    impl ActiveModelBehavior for ActiveModel {}
}

pub mod models {
    use sea_orm::{entity::prelude::*, FromQueryResult};

    #[derive(Clone, Debug, PartialEq, DeriveEntityModel)]
    #[sea_orm(table_name = "models")]
    pub struct Model {
        #[sea_orm(primary_key)]
        pub id: i64,
        pub created_at: String,
        pub updated_at: String,
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
        pub remark: Option<String>,
        pub deleted_at: i64,
    }

    #[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
    pub enum Relation {}

    #[derive(Clone, Debug, PartialEq, Eq, DerivePartialModel, FromQueryResult)]
    #[sea_orm(entity = "Entity")]
    pub struct EnabledModelRecord {
        pub id: i64,
        #[sea_orm(from_expr = "Expr::col(Column::CreatedAt).cast_as(\"text\")")]
        pub created_at: String,
        pub developer: String,
        pub model_id: String,
        #[sea_orm(from_col = "type_field")]
        pub model_type: String,
        pub name: String,
        pub icon: String,
        pub remark: Option<String>,
        pub model_card: String,
    }

    #[derive(Clone, Debug, PartialEq, Eq, DerivePartialModel, FromQueryResult)]
    #[sea_orm(entity = "Entity")]
    pub struct GraphqlStatus {
        pub id: i64,
        pub status: String,
    }

    impl ActiveModelBehavior for ActiveModel {}
}

pub mod projects {
    use sea_orm::{entity::prelude::*, FromQueryResult};

    #[derive(Clone, Debug, PartialEq, DeriveEntityModel)]
    #[sea_orm(table_name = "projects")]
    pub struct Model {
        #[sea_orm(primary_key)]
        pub id: i64,
        pub created_at: String,
        pub updated_at: String,
        pub name: String,
        pub description: String,
        pub status: String,
        pub deleted_at: i64,
    }

    #[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
    pub enum Relation {
        #[sea_orm(has_many = "super::api_keys::Entity")]
        ApiKeys,
        #[sea_orm(has_many = "super::operational_runs::Entity")]
        OperationalRuns,
        #[sea_orm(has_many = "super::prompts::Entity")]
        Prompts,
        #[sea_orm(has_many = "super::realtime_sessions::Entity")]
        RealtimeSessions,
        #[sea_orm(has_many = "super::request_executions::Entity")]
        RequestExecutions,
        #[sea_orm(has_many = "super::requests::Entity")]
        Requests,
        #[sea_orm(has_many = "super::roles::Entity")]
        Roles,
        #[sea_orm(has_many = "super::threads::Entity")]
        Threads,
        #[sea_orm(has_many = "super::traces::Entity")]
        Traces,
        #[sea_orm(has_many = "super::usage_logs::Entity")]
        UsageLogs,
        #[sea_orm(has_many = "super::user_projects::Entity")]
        UserProjects,
    }

    impl Related<super::api_keys::Entity> for Entity {
        fn to() -> RelationDef {
            Relation::ApiKeys.def()
        }
    }

    impl Related<super::operational_runs::Entity> for Entity {
        fn to() -> RelationDef {
            Relation::OperationalRuns.def()
        }
    }

    impl Related<super::prompts::Entity> for Entity {
        fn to() -> RelationDef {
            Relation::Prompts.def()
        }
    }

    impl Related<super::realtime_sessions::Entity> for Entity {
        fn to() -> RelationDef {
            Relation::RealtimeSessions.def()
        }
    }

    impl Related<super::request_executions::Entity> for Entity {
        fn to() -> RelationDef {
            Relation::RequestExecutions.def()
        }
    }

    impl Related<super::requests::Entity> for Entity {
        fn to() -> RelationDef {
            Relation::Requests.def()
        }
    }

    impl Related<super::roles::Entity> for Entity {
        fn to() -> RelationDef {
            Relation::Roles.def()
        }
    }

    impl Related<super::threads::Entity> for Entity {
        fn to() -> RelationDef {
            Relation::Threads.def()
        }
    }

    impl Related<super::traces::Entity> for Entity {
        fn to() -> RelationDef {
            Relation::Traces.def()
        }
    }

    impl Related<super::usage_logs::Entity> for Entity {
        fn to() -> RelationDef {
            Relation::UsageLogs.def()
        }
    }

    impl Related<super::user_projects::Entity> for Entity {
        fn to() -> RelationDef {
            Relation::UserProjects.def()
        }
    }

    #[derive(Clone, Debug, PartialEq, Eq, DerivePartialModel, FromQueryResult)]
    #[sea_orm(entity = "Entity")]
    pub struct ContextSummary {
        pub id: i64,
        pub name: String,
        pub status: String,
    }

    #[derive(Clone, Debug, PartialEq, Eq, DerivePartialModel, FromQueryResult)]
    #[sea_orm(entity = "Entity")]
    pub struct MembershipSummary {
        pub id: i64,
        pub name: String,
    }

    impl ActiveModelBehavior for ActiveModel {}
}

pub mod prompt_protection_rules {
    use sea_orm::entity::prelude::*;

    #[derive(Clone, Debug, PartialEq, DeriveEntityModel)]
    #[sea_orm(table_name = "prompt_protection_rules")]
    pub struct Model {
        #[sea_orm(primary_key)]
        pub id: i64,
        pub created_at: String,
        pub updated_at: String,
        pub name: String,
        pub description: String,
        pub pattern: String,
        pub status: String,
        pub settings: String,
        pub deleted_at: i64,
    }

    #[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
    pub enum Relation {}

    impl ActiveModelBehavior for ActiveModel {}
}

pub mod realtime_sessions {
    use sea_orm::entity::prelude::*;

    #[derive(Clone, Debug, PartialEq, DeriveEntityModel)]
    #[sea_orm(table_name = "realtime_sessions")]
    pub struct Model {
        #[sea_orm(primary_key)]
        pub id: i64,
        pub project_id: i64,
        pub thread_id: Option<i64>,
        pub trace_id: Option<i64>,
        pub request_id: Option<i64>,
        pub api_key_id: Option<i64>,
        pub channel_id: Option<i64>,
        pub session_id: String,
        pub transport: String,
        pub status: String,
        pub metadata: String,
        pub opened_at: String,
        pub last_activity_at: String,
        pub closed_at: Option<String>,
        pub expires_at: Option<String>,
    }

    #[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
    pub enum Relation {
        #[sea_orm(
            belongs_to = "super::projects::Entity",
            from = "Column::ProjectId",
            to = "super::projects::Column::Id"
        )]
        Projects,
        #[sea_orm(
            belongs_to = "super::threads::Entity",
            from = "Column::ThreadId",
            to = "super::threads::Column::Id"
        )]
        Threads,
        #[sea_orm(
            belongs_to = "super::traces::Entity",
            from = "Column::TraceId",
            to = "super::traces::Column::Id"
        )]
        Traces,
        #[sea_orm(
            belongs_to = "super::requests::Entity",
            from = "Column::RequestId",
            to = "super::requests::Column::Id"
        )]
        Requests,
        #[sea_orm(
            belongs_to = "super::api_keys::Entity",
            from = "Column::ApiKeyId",
            to = "super::api_keys::Column::Id"
        )]
        ApiKeys,
        #[sea_orm(
            belongs_to = "super::channels::Entity",
            from = "Column::ChannelId",
            to = "super::channels::Column::Id"
        )]
        Channels,
    }

    impl Related<super::projects::Entity> for Entity {
        fn to() -> RelationDef {
            Relation::Projects.def()
        }
    }

    impl Related<super::threads::Entity> for Entity {
        fn to() -> RelationDef {
            Relation::Threads.def()
        }
    }

    impl Related<super::traces::Entity> for Entity {
        fn to() -> RelationDef {
            Relation::Traces.def()
        }
    }

    impl Related<super::requests::Entity> for Entity {
        fn to() -> RelationDef {
            Relation::Requests.def()
        }
    }

    impl Related<super::api_keys::Entity> for Entity {
        fn to() -> RelationDef {
            Relation::ApiKeys.def()
        }
    }

    impl Related<super::channels::Entity> for Entity {
        fn to() -> RelationDef {
            Relation::Channels.def()
        }
    }

    impl ActiveModelBehavior for ActiveModel {}
}

pub mod prompts {
    use sea_orm::entity::prelude::*;

    #[derive(Clone, Debug, PartialEq, DeriveEntityModel)]
    #[sea_orm(table_name = "prompts")]
    pub struct Model {
        #[sea_orm(primary_key)]
        pub id: i64,
        pub created_at: String,
        pub updated_at: String,
        pub project_id: i64,
        pub name: String,
        pub description: String,
        pub role: String,
        pub content: String,
        pub status: String,
        pub order: i32,
        pub settings: String,
        pub deleted_at: i64,
    }

    #[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
    pub enum Relation {
        #[sea_orm(
            belongs_to = "super::projects::Entity",
            from = "Column::ProjectId",
            to = "super::projects::Column::Id"
        )]
        Projects,
    }

    impl Related<super::projects::Entity> for Entity {
        fn to() -> RelationDef {
            Relation::Projects.def()
        }
    }

    impl ActiveModelBehavior for ActiveModel {}
}

pub mod provider_quota_statuses {
    use sea_orm::entity::prelude::*;

    #[derive(Clone, Debug, PartialEq, DeriveEntityModel)]
    #[sea_orm(table_name = "provider_quota_statuses")]
    pub struct Model {
        #[sea_orm(primary_key)]
        pub id: i64,
        pub created_at: String,
        pub updated_at: String,
        pub deleted_at: i64,
        pub channel_id: i64,
        pub provider_type: String,
        pub status: String,
        pub quota_data: String,
        pub next_reset_at: Option<String>,
        pub ready: bool,
        pub next_check_at: String,
    }

    #[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
    pub enum Relation {
        #[sea_orm(
            belongs_to = "super::channels::Entity",
            from = "Column::ChannelId",
            to = "super::channels::Column::Id"
        )]
        Channels,
    }

    impl Related<super::channels::Entity> for Entity {
        fn to() -> RelationDef {
            Relation::Channels.def()
        }
    }

    impl ActiveModelBehavior for ActiveModel {}
}

pub mod request_executions {
    use sea_orm::{entity::prelude::*, FromQueryResult};

    #[derive(Clone, Debug, PartialEq, DeriveEntityModel)]
    #[sea_orm(table_name = "request_executions")]
    pub struct Model {
        #[sea_orm(primary_key)]
        pub id: i64,
        pub created_at: String,
        pub updated_at: String,
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
        pub error_message: Option<String>,
        pub response_status_code: Option<i64>,
        pub status: String,
        pub stream: bool,
        pub metrics_latency_ms: Option<i64>,
        pub metrics_first_token_latency_ms: Option<i64>,
        pub request_headers: Option<String>,
    }

    #[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
    pub enum Relation {
        #[sea_orm(
            belongs_to = "super::channels::Entity",
            from = "Column::ChannelId",
            to = "super::channels::Column::Id"
        )]
        Channels,
        #[sea_orm(
            belongs_to = "super::data_storages::Entity",
            from = "Column::DataStorageId",
            to = "super::data_storages::Column::Id"
        )]
        DataStorages,
        #[sea_orm(
            belongs_to = "super::projects::Entity",
            from = "Column::ProjectId",
            to = "super::projects::Column::Id"
        )]
        Projects,
        #[sea_orm(
            belongs_to = "super::requests::Entity",
            from = "Column::RequestId",
            to = "super::requests::Column::Id"
        )]
        Requests,
    }

    impl Related<super::channels::Entity> for Entity {
        fn to() -> RelationDef {
            Relation::Channels.def()
        }
    }

    impl Related<super::data_storages::Entity> for Entity {
        fn to() -> RelationDef {
            Relation::DataStorages.def()
        }
    }

    impl Related<super::projects::Entity> for Entity {
        fn to() -> RelationDef {
            Relation::Projects.def()
        }
    }

    impl Related<super::requests::Entity> for Entity {
        fn to() -> RelationDef {
            Relation::Requests.def()
        }
    }

    #[derive(Clone, Debug, PartialEq, Eq, DerivePartialModel, FromQueryResult)]
    #[sea_orm(entity = "Entity")]
    pub struct StatusOnly {
        pub status: String,
    }

    impl ActiveModelBehavior for ActiveModel {}
}

pub mod requests {
    use sea_orm::{entity::prelude::*, FromQueryResult};

    #[derive(Clone, Debug, PartialEq, DeriveEntityModel)]
    #[sea_orm(table_name = "requests")]
    pub struct Model {
        #[sea_orm(primary_key)]
        pub id: i64,
        pub created_at: String,
        pub updated_at: String,
        pub api_key_id: Option<i64>,
        pub project_id: i64,
        pub trace_id: Option<i64>,
        pub data_storage_id: Option<i64>,
        pub source: String,
        pub model_id: String,
        pub format: String,
        pub request_headers: Option<String>,
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
    pub enum Relation {
        #[sea_orm(
            belongs_to = "super::api_keys::Entity",
            from = "Column::ApiKeyId",
            to = "super::api_keys::Column::Id"
        )]
        ApiKeys,
        #[sea_orm(
            belongs_to = "super::channels::Entity",
            from = "Column::ChannelId",
            to = "super::channels::Column::Id"
        )]
        Channels,
        #[sea_orm(
            belongs_to = "super::data_storages::Entity",
            from = "Column::DataStorageId",
            to = "super::data_storages::Column::Id"
        )]
        DataStorages,
        #[sea_orm(
            belongs_to = "super::projects::Entity",
            from = "Column::ProjectId",
            to = "super::projects::Column::Id"
        )]
        Projects,
        #[sea_orm(has_many = "super::request_executions::Entity")]
        RequestExecutions,
        #[sea_orm(
            belongs_to = "super::traces::Entity",
            from = "Column::TraceId",
            to = "super::traces::Column::Id"
        )]
        Traces,
        #[sea_orm(has_many = "super::usage_logs::Entity")]
        UsageLogs,
        #[sea_orm(has_many = "super::realtime_sessions::Entity")]
        RealtimeSessions,
    }

    impl Related<super::api_keys::Entity> for Entity {
        fn to() -> RelationDef {
            Relation::ApiKeys.def()
        }
    }

    impl Related<super::channels::Entity> for Entity {
        fn to() -> RelationDef {
            Relation::Channels.def()
        }
    }

    impl Related<super::data_storages::Entity> for Entity {
        fn to() -> RelationDef {
            Relation::DataStorages.def()
        }
    }

    impl Related<super::projects::Entity> for Entity {
        fn to() -> RelationDef {
            Relation::Projects.def()
        }
    }

    impl Related<super::request_executions::Entity> for Entity {
        fn to() -> RelationDef {
            Relation::RequestExecutions.def()
        }
    }

    impl Related<super::traces::Entity> for Entity {
        fn to() -> RelationDef {
            Relation::Traces.def()
        }
    }

    impl Related<super::usage_logs::Entity> for Entity {
        fn to() -> RelationDef {
            Relation::UsageLogs.def()
        }
    }

    impl Related<super::realtime_sessions::Entity> for Entity {
        fn to() -> RelationDef {
            Relation::RealtimeSessions.def()
        }
    }

    #[derive(Clone, Debug, PartialEq, Eq, DerivePartialModel, FromQueryResult)]
    #[sea_orm(entity = "Entity")]
    pub struct ContentStorageLookup {
        pub id: i64,
        pub project_id: i64,
        pub content_saved: bool,
        pub content_storage_id: Option<i64>,
        pub content_storage_key: Option<String>,
    }

    #[derive(Clone, Debug, PartialEq, Eq, DerivePartialModel, FromQueryResult)]
    #[sea_orm(entity = "Entity")]
    pub struct RouteHint {
        pub channel_id: Option<i64>,
        pub model_id: String,
    }

    #[derive(Clone, Debug, PartialEq, Eq, DerivePartialModel, FromQueryResult)]
    #[sea_orm(entity = "Entity")]
    pub struct TraceChannelAffinity {
        pub channel_id: Option<i64>,
    }

    #[derive(Clone, Debug, PartialEq, Eq, DerivePartialModel, FromQueryResult)]
    #[sea_orm(entity = "Entity")]
    pub struct ChannelSelectionCount {
        pub id: i64,
    }

    impl ActiveModelBehavior for ActiveModel {}
}

pub mod roles {
    use sea_orm::{entity::prelude::*, FromQueryResult};

    #[derive(Clone, Debug, PartialEq, DeriveEntityModel)]
    #[sea_orm(table_name = "roles")]
    pub struct Model {
        #[sea_orm(primary_key)]
        pub id: i64,
        pub created_at: String,
        pub updated_at: String,
        pub name: String,
        pub level: String,
        pub project_id: i64,
        pub scopes: String,
        pub deleted_at: i64,
    }

    #[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
    pub enum Relation {
        #[sea_orm(
            belongs_to = "super::projects::Entity",
            from = "Column::ProjectId",
            to = "super::projects::Column::Id"
        )]
        Projects,
        #[sea_orm(has_many = "super::user_roles::Entity")]
        UserRoles,
    }

    impl Related<super::projects::Entity> for Entity {
        fn to() -> RelationDef {
            Relation::Projects.def()
        }
    }

    impl Related<super::user_roles::Entity> for Entity {
        fn to() -> RelationDef {
            Relation::UserRoles.def()
        }
    }

    impl Related<super::users::Entity> for Entity {
        fn to() -> RelationDef {
            super::user_roles::Relation::Users.def()
        }

        fn via() -> Option<RelationDef> {
            Some(super::user_roles::Relation::Roles.def().rev())
        }
    }

    #[derive(Clone, Debug, PartialEq, Eq, DerivePartialModel, FromQueryResult)]
    #[sea_orm(entity = "Entity")]
    pub struct Assignment {
        pub id: i64,
        pub name: String,
        pub level: String,
        pub project_id: i64,
        pub scopes: String,
    }

    #[derive(Clone, Debug, PartialEq, Eq, DerivePartialModel, FromQueryResult)]
    #[sea_orm(entity = "Entity")]
    pub struct GraphqlRoleSummary {
        pub id: i64,
        pub name: String,
        pub scopes: String,
    }

    impl ActiveModelBehavior for ActiveModel {}
}

pub mod operational_runs {
    use sea_orm::entity::prelude::*;

    #[derive(Clone, Debug, PartialEq, DeriveEntityModel)]
    #[sea_orm(table_name = "operational_runs")]
    pub struct Model {
        #[sea_orm(primary_key)]
        pub id: i64,
        pub operation_type: String,
        pub trigger_source: String,
        pub status: String,
        pub result_payload: Option<String>,
        pub error_message: Option<String>,
        pub initiated_by_user_id: Option<i64>,
        pub data_storage_id: Option<i64>,
        pub channel_id: Option<i64>,
        pub project_id: Option<i64>,
        pub started_at: String,
        pub finished_at: Option<String>,
    }

    #[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
    pub enum Relation {
        #[sea_orm(
            belongs_to = "super::users::Entity",
            from = "Column::InitiatedByUserId",
            to = "super::users::Column::Id"
        )]
        Users,
        #[sea_orm(
            belongs_to = "super::data_storages::Entity",
            from = "Column::DataStorageId",
            to = "super::data_storages::Column::Id"
        )]
        DataStorages,
        #[sea_orm(
            belongs_to = "super::channels::Entity",
            from = "Column::ChannelId",
            to = "super::channels::Column::Id"
        )]
        Channels,
        #[sea_orm(
            belongs_to = "super::projects::Entity",
            from = "Column::ProjectId",
            to = "super::projects::Column::Id"
        )]
        Projects,
    }

    impl Related<super::users::Entity> for Entity {
        fn to() -> RelationDef {
            Relation::Users.def()
        }
    }

    impl Related<super::data_storages::Entity> for Entity {
        fn to() -> RelationDef {
            Relation::DataStorages.def()
        }
    }

    impl Related<super::channels::Entity> for Entity {
        fn to() -> RelationDef {
            Relation::Channels.def()
        }
    }

    impl Related<super::projects::Entity> for Entity {
        fn to() -> RelationDef {
            Relation::Projects.def()
        }
    }

    impl ActiveModelBehavior for ActiveModel {}
}

pub mod systems {
    use sea_orm::{entity::prelude::*, FromQueryResult};

    #[derive(Clone, Debug, PartialEq, DeriveEntityModel)]
    #[sea_orm(table_name = "systems")]
    pub struct Model {
        #[sea_orm(primary_key)]
        pub id: i64,
        pub created_at: String,
        pub updated_at: String,
        pub key: String,
        pub value: String,
        pub deleted_at: i64,
    }

    #[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
    pub enum Relation {}

    #[derive(Clone, Debug, PartialEq, Eq, DerivePartialModel, FromQueryResult)]
    #[sea_orm(entity = "Entity")]
    pub struct KeyValue {
        pub value: String,
    }

    impl ActiveModelBehavior for ActiveModel {}
}

pub mod threads {
    use sea_orm::{entity::prelude::*, FromQueryResult};

    #[derive(Clone, Debug, PartialEq, DeriveEntityModel)]
    #[sea_orm(table_name = "threads")]
    pub struct Model {
        #[sea_orm(primary_key)]
        pub id: i64,
        pub created_at: String,
        pub updated_at: String,
        pub project_id: i64,
        pub thread_id: String,
    }

    #[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
    pub enum Relation {
        #[sea_orm(
            belongs_to = "super::projects::Entity",
            from = "Column::ProjectId",
            to = "super::projects::Column::Id"
        )]
        Projects,
        #[sea_orm(has_many = "super::traces::Entity")]
        Traces,
        #[sea_orm(has_many = "super::realtime_sessions::Entity")]
        RealtimeSessions,
    }

    impl Related<super::projects::Entity> for Entity {
        fn to() -> RelationDef {
            Relation::Projects.def()
        }
    }

    impl Related<super::traces::Entity> for Entity {
        fn to() -> RelationDef {
            Relation::Traces.def()
        }
    }

    impl Related<super::realtime_sessions::Entity> for Entity {
        fn to() -> RelationDef {
            Relation::RealtimeSessions.def()
        }
    }

    #[derive(Clone, Debug, PartialEq, Eq, DerivePartialModel, FromQueryResult)]
    #[sea_orm(entity = "Entity")]
    pub struct ResolveContext {
        pub id: i64,
        pub thread_id: String,
        pub project_id: i64,
    }

    impl ActiveModelBehavior for ActiveModel {}
}

pub mod traces {
    use sea_orm::{entity::prelude::*, FromQueryResult};

    #[derive(Clone, Debug, PartialEq, DeriveEntityModel)]
    #[sea_orm(table_name = "traces")]
    pub struct Model {
        #[sea_orm(primary_key)]
        pub id: i64,
        pub created_at: String,
        pub updated_at: String,
        pub project_id: i64,
        pub trace_id: String,
        pub thread_id: Option<i64>,
    }

    #[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
    pub enum Relation {
        #[sea_orm(
            belongs_to = "super::projects::Entity",
            from = "Column::ProjectId",
            to = "super::projects::Column::Id"
        )]
        Projects,
        #[sea_orm(has_many = "super::requests::Entity")]
        Requests,
        #[sea_orm(
            belongs_to = "super::threads::Entity",
            from = "Column::ThreadId",
            to = "super::threads::Column::Id"
        )]
        Threads,
        #[sea_orm(has_many = "super::realtime_sessions::Entity")]
        RealtimeSessions,
    }

    impl Related<super::projects::Entity> for Entity {
        fn to() -> RelationDef {
            Relation::Projects.def()
        }
    }

    impl Related<super::requests::Entity> for Entity {
        fn to() -> RelationDef {
            Relation::Requests.def()
        }
    }

    impl Related<super::threads::Entity> for Entity {
        fn to() -> RelationDef {
            Relation::Threads.def()
        }
    }

    impl Related<super::realtime_sessions::Entity> for Entity {
        fn to() -> RelationDef {
            Relation::RealtimeSessions.def()
        }
    }

    #[derive(Clone, Debug, PartialEq, Eq, DerivePartialModel, FromQueryResult)]
    #[sea_orm(entity = "Entity")]
    pub struct ResolveContext {
        pub id: i64,
        pub trace_id: String,
        pub project_id: i64,
        pub thread_id: Option<i64>,
    }

    impl ActiveModelBehavior for ActiveModel {}
}

pub mod usage_logs {
    use sea_orm::entity::prelude::*;

    #[derive(Clone, Debug, PartialEq, DeriveEntityModel)]
    #[sea_orm(table_name = "usage_logs")]
    pub struct Model {
        #[sea_orm(primary_key)]
        pub id: i64,
        pub created_at: String,
        pub updated_at: String,
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
        pub cost_price_reference_id: Option<String>,
    }

    #[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
    pub enum Relation {
        #[sea_orm(
            belongs_to = "super::api_keys::Entity",
            from = "Column::ApiKeyId",
            to = "super::api_keys::Column::Id"
        )]
        ApiKeys,
        #[sea_orm(
            belongs_to = "super::channels::Entity",
            from = "Column::ChannelId",
            to = "super::channels::Column::Id"
        )]
        Channels,
        #[sea_orm(
            belongs_to = "super::projects::Entity",
            from = "Column::ProjectId",
            to = "super::projects::Column::Id"
        )]
        Projects,
        #[sea_orm(
            belongs_to = "super::requests::Entity",
            from = "Column::RequestId",
            to = "super::requests::Column::Id"
        )]
        Requests,
    }

    impl Related<super::api_keys::Entity> for Entity {
        fn to() -> RelationDef {
            Relation::ApiKeys.def()
        }
    }

    impl Related<super::channels::Entity> for Entity {
        fn to() -> RelationDef {
            Relation::Channels.def()
        }
    }

    impl Related<super::projects::Entity> for Entity {
        fn to() -> RelationDef {
            Relation::Projects.def()
        }
    }

    impl Related<super::requests::Entity> for Entity {
        fn to() -> RelationDef {
            Relation::Requests.def()
        }
    }

    impl ActiveModelBehavior for ActiveModel {}
}

pub mod user_projects {
    use sea_orm::{entity::prelude::*, FromQueryResult};

    #[derive(Clone, Debug, PartialEq, DeriveEntityModel)]
    #[sea_orm(table_name = "user_projects")]
    pub struct Model {
        #[sea_orm(primary_key)]
        pub id: i64,
        pub created_at: String,
        pub updated_at: String,
        pub user_id: i64,
        pub project_id: i64,
        pub is_owner: bool,
        pub scopes: String,
    }

    #[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
    pub enum Relation {
        #[sea_orm(
            belongs_to = "super::projects::Entity",
            from = "Column::ProjectId",
            to = "super::projects::Column::Id"
        )]
        Projects,
        #[sea_orm(
            belongs_to = "super::users::Entity",
            from = "Column::UserId",
            to = "super::users::Column::Id"
        )]
        Users,
    }

    impl Related<super::projects::Entity> for Entity {
        fn to() -> RelationDef {
            Relation::Projects.def()
        }
    }

    impl Related<super::users::Entity> for Entity {
        fn to() -> RelationDef {
            Relation::Users.def()
        }
    }

    #[derive(Clone, Debug, PartialEq, Eq, DerivePartialModel, FromQueryResult)]
    #[sea_orm(entity = "Entity")]
    pub struct MembershipLink {
        pub project_id: i64,
        pub is_owner: bool,
        pub scopes: String,
    }

    #[derive(Clone, Debug, PartialEq, Eq, DerivePartialModel, FromQueryResult)]
    #[sea_orm(entity = "Entity")]
    pub struct GraphqlMembership {
        pub project_id: i64,
        pub is_owner: bool,
        pub scopes: String,
    }

    impl ActiveModelBehavior for ActiveModel {}
}

pub mod user_roles {
    use sea_orm::entity::prelude::*;

    #[derive(Clone, Debug, PartialEq, DeriveEntityModel)]
    #[sea_orm(table_name = "user_roles")]
    pub struct Model {
        #[sea_orm(primary_key)]
        pub id: i64,
        pub created_at: String,
        pub updated_at: String,
        pub user_id: i64,
        pub role_id: i64,
    }

    #[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
    pub enum Relation {
        #[sea_orm(
            belongs_to = "super::roles::Entity",
            from = "Column::RoleId",
            to = "super::roles::Column::Id"
        )]
        Roles,
        #[sea_orm(
            belongs_to = "super::users::Entity",
            from = "Column::UserId",
            to = "super::users::Column::Id"
        )]
        Users,
    }

    impl Related<super::roles::Entity> for Entity {
        fn to() -> RelationDef {
            Relation::Roles.def()
        }
    }

    impl Related<super::users::Entity> for Entity {
        fn to() -> RelationDef {
            Relation::Users.def()
        }
    }

    impl ActiveModelBehavior for ActiveModel {}
}

pub mod users {
    use sea_orm::{entity::prelude::*, FromQueryResult};

    #[derive(Clone, Debug, PartialEq, DeriveEntityModel)]
    #[sea_orm(table_name = "users")]
    pub struct Model {
        #[sea_orm(primary_key)]
        pub id: i64,
        pub created_at: String,
        pub updated_at: String,
        pub email: String,
        pub status: String,
        pub prefer_language: String,
        pub password: String,
        pub first_name: String,
        pub last_name: String,
        pub avatar: Option<String>,
        pub is_owner: bool,
        pub scopes: String,
        pub deleted_at: i64,
    }

    #[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
    pub enum Relation {
        #[sea_orm(has_many = "super::api_keys::Entity")]
        ApiKeys,
        #[sea_orm(has_many = "super::channel_override_templates::Entity")]
        ChannelOverrideTemplates,
        #[sea_orm(has_many = "super::operational_runs::Entity")]
        OperationalRuns,
        #[sea_orm(has_many = "super::user_projects::Entity")]
        UserProjects,
        #[sea_orm(has_many = "super::user_roles::Entity")]
        UserRoles,
    }

    impl Related<super::api_keys::Entity> for Entity {
        fn to() -> RelationDef {
            Relation::ApiKeys.def()
        }
    }

    impl Related<super::channel_override_templates::Entity> for Entity {
        fn to() -> RelationDef {
            Relation::ChannelOverrideTemplates.def()
        }
    }

    impl Related<super::operational_runs::Entity> for Entity {
        fn to() -> RelationDef {
            Relation::OperationalRuns.def()
        }
    }

    impl Related<super::user_projects::Entity> for Entity {
        fn to() -> RelationDef {
            Relation::UserProjects.def()
        }
    }

    impl Related<super::user_roles::Entity> for Entity {
        fn to() -> RelationDef {
            Relation::UserRoles.def()
        }
    }

    impl Related<super::projects::Entity> for Entity {
        fn to() -> RelationDef {
            super::user_projects::Relation::Projects.def()
        }

        fn via() -> Option<RelationDef> {
            Some(super::user_projects::Relation::Users.def().rev())
        }
    }

    impl Related<super::roles::Entity> for Entity {
        fn to() -> RelationDef {
            super::user_roles::Relation::Roles.def()
        }

        fn via() -> Option<RelationDef> {
            Some(super::user_roles::Relation::Users.def().rev())
        }
    }

    #[derive(Clone, Debug, PartialEq, Eq, DerivePartialModel, FromQueryResult)]
    #[sea_orm(entity = "Entity")]
    pub struct AuthLookup {
        pub id: i64,
        pub email: String,
        pub status: String,
        pub prefer_language: String,
        pub password: String,
        pub first_name: String,
        pub last_name: String,
        pub avatar: Option<String>,
        pub is_owner: bool,
        pub scopes: String,
    }

    #[derive(Clone, Debug, PartialEq, Eq, DerivePartialModel, FromQueryResult)]
    #[sea_orm(entity = "Entity")]
    pub struct GraphqlProfile {
        pub id: i64,
        pub email: String,
        pub first_name: String,
        pub last_name: String,
        pub is_owner: bool,
        pub prefer_language: String,
        pub avatar: Option<String>,
        pub scopes: String,
    }

    #[derive(Clone, Debug, PartialEq, Eq, DerivePartialModel, FromQueryResult)]
    #[sea_orm(entity = "Entity")]
    pub struct GraphqlUserListItem {
        pub id: i64,
        pub email: String,
        pub first_name: String,
        pub last_name: String,
        pub is_owner: bool,
        pub prefer_language: String,
        pub status: String,
        pub created_at: String,
        pub updated_at: String,
        pub scopes: String,
    }

    impl ActiveModelBehavior for ActiveModel {}
}

#[cfg(test)]
mod tests {
    use super::*;
    use sea_orm::{EntityName, Iden, Related, RelationTrait};

    #[test]
    fn users_have_role_and_project_relations() {
        let role_relation = <users::Entity as Related<roles::Entity>>::to();
        assert_eq!(role_relation.to_tbl, roles::Entity.table_ref());
        assert!(<users::Entity as Related<roles::Entity>>::via().is_some());

        let project_relation = <users::Entity as Related<projects::Entity>>::to();
        assert_eq!(project_relation.to_tbl, projects::Entity.table_ref());
        assert!(<users::Entity as Related<projects::Entity>>::via().is_some());
    }

    #[test]
    fn request_and_trace_thread_relations_target_expected_tables() {
        assert_eq!(
            requests::Relation::Traces.def().to_tbl,
            traces::Entity.table_ref()
        );
        assert_eq!(
            traces::Relation::Threads.def().to_tbl,
            threads::Entity.table_ref()
        );
        assert_eq!(
            requests::Relation::DataStorages.def().to_tbl,
            data_storages::Entity.table_ref()
        );
        assert_eq!(
            requests::Relation::RequestExecutions.def().to_tbl,
            request_executions::Entity.table_ref()
        );
    }

    #[test]
    fn channel_operational_relations_cover_lifecycle_tables() {
        assert_eq!(
            channels::Relation::Requests.def().to_tbl,
            requests::Entity.table_ref()
        );
        assert_eq!(
            channels::Relation::RequestExecutions.def().to_tbl,
            request_executions::Entity.table_ref()
        );
        assert_eq!(
            channels::Relation::UsageLogs.def().to_tbl,
            usage_logs::Entity.table_ref()
        );
        assert_eq!(
            channels::Relation::ChannelProbes.def().to_tbl,
            channel_probes::Entity.table_ref()
        );
        assert_eq!(
            channels::Relation::ChannelModelPrices.def().to_tbl,
            channel_model_prices::Entity.table_ref()
        );
        assert_eq!(
            <users::Entity as Related<operational_runs::Entity>>::to().to_tbl,
            operational_runs::Entity.table_ref()
        );
    }

    #[test]
    fn prompt_and_override_relations_target_expected_tables() {
        assert_eq!(
            prompts::Relation::Projects.def().to_tbl,
            projects::Entity.table_ref()
        );
        assert_eq!(
            <projects::Entity as Related<prompts::Entity>>::to().to_tbl,
            prompts::Entity.table_ref()
        );
        assert_eq!(
            channel_override_templates::Relation::Users.def().to_tbl,
            users::Entity.table_ref()
        );
        assert_eq!(
            <users::Entity as Related<channel_override_templates::Entity>>::to().to_tbl,
            channel_override_templates::Entity.table_ref()
        );
        assert_eq!(
            <projects::Entity as Related<realtime_sessions::Entity>>::to().to_tbl,
            realtime_sessions::Entity.table_ref()
        );
    }

    #[test]
    fn realtime_and_operational_run_relations_target_expected_tables() {
        assert_eq!(
            realtime_sessions::Relation::Projects.def().to_tbl,
            projects::Entity.table_ref()
        );
        assert_eq!(
            realtime_sessions::Relation::Threads.def().to_tbl,
            threads::Entity.table_ref()
        );
        assert_eq!(
            realtime_sessions::Relation::Traces.def().to_tbl,
            traces::Entity.table_ref()
        );
        assert_eq!(
            realtime_sessions::Relation::Requests.def().to_tbl,
            requests::Entity.table_ref()
        );
        assert_eq!(
            operational_runs::Relation::Users.def().to_tbl,
            users::Entity.table_ref()
        );
        assert_eq!(
            operational_runs::Relation::DataStorages.def().to_tbl,
            data_storages::Entity.table_ref()
        );
        assert_eq!(
            operational_runs::Relation::Channels.def().to_tbl,
            channels::Entity.table_ref()
        );
        assert_eq!(
            operational_runs::Relation::Projects.def().to_tbl,
            projects::Entity.table_ref()
        );
    }

    #[test]
    fn channel_price_relations_connect_parent_and_versions() {
        assert_eq!(
            channel_model_prices::Relation::Channels.def().to_tbl,
            channels::Entity.table_ref()
        );
        assert_eq!(
            channel_model_prices::Relation::ChannelModelPriceVersions
                .def()
                .to_tbl,
            channel_model_price_versions::Entity.table_ref()
        );
        assert_eq!(
            channel_model_price_versions::Relation::ChannelModelPrices
                .def()
                .to_tbl,
            channel_model_prices::Entity.table_ref()
        );
    }

    #[test]
    fn partial_models_keep_query_inventory_projection_columns() {
        assert_eq!(
            requests::Column::ContentStorageKey.to_string(),
            "content_storage_key"
        );
        assert_eq!(data_storages::Column::TypeField.to_string(), "type");
        assert_eq!(api_keys::Column::TypeField.to_string(), "type");
        assert_eq!(models::Column::TypeField.to_string(), "type");
        assert_eq!(
            channel_override_templates::Column::BodyOverrideOperations.to_string(),
            "body_override_operations"
        );
        assert_eq!(
            channel_model_price_versions::Column::EffectiveStartAt.to_string(),
            "effective_start_at"
        );
    }
}
