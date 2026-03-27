use sea_orm::DatabaseBackend;
use sea_orm_migration::prelude::*;

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        let backend = manager.get_database_backend();

        let mut requests_created_at = ColumnDef::new(General::CreatedAt);
        match backend {
            DatabaseBackend::Sqlite => requests_created_at.custom(Alias::new("TEXT")),
            DatabaseBackend::Postgres => requests_created_at.timestamp_with_time_zone(),
            DatabaseBackend::MySql => requests_created_at.timestamp(),
        };
        requests_created_at
            .not_null()
            .default(Expr::current_timestamp());

        let mut requests_updated_at = ColumnDef::new(General::UpdatedAt);
        match backend {
            DatabaseBackend::Sqlite => requests_updated_at.custom(Alias::new("TEXT")),
            DatabaseBackend::Postgres => requests_updated_at.timestamp_with_time_zone(),
            DatabaseBackend::MySql => requests_updated_at.timestamp(),
        };
        requests_updated_at
            .not_null()
            .default(Expr::current_timestamp());
        if matches!(backend, DatabaseBackend::MySql) {
            requests_updated_at.extra("ON UPDATE CURRENT_TIMESTAMP");
        }

        let mut requests_source = ColumnDef::new(Requests::Source);
        requests_source.text().not_null();
        if !matches!(backend, DatabaseBackend::MySql) {
            requests_source.default("api");
        }

        let mut requests_format = ColumnDef::new(Requests::Format);
        requests_format.text().not_null();
        if !matches!(backend, DatabaseBackend::MySql) {
            requests_format.default("openai/chat_completions");
        }

        let mut requests_request_headers = ColumnDef::new(Requests::RequestHeaders);
        if matches!(backend, DatabaseBackend::MySql) {
            requests_request_headers.custom(Alias::new("LONGTEXT"));
        } else {
            requests_request_headers.text();
        }
        requests_request_headers.null();

        let mut requests_request_body = ColumnDef::new(Requests::RequestBody);
        if matches!(backend, DatabaseBackend::MySql) {
            requests_request_body.custom(Alias::new("LONGTEXT"));
        } else {
            requests_request_body.text();
            requests_request_body.default("{}");
        }
        requests_request_body.not_null();

        let mut requests_response_body = ColumnDef::new(Requests::ResponseBody);
        if matches!(backend, DatabaseBackend::MySql) {
            requests_response_body.custom(Alias::new("LONGTEXT"));
        } else {
            requests_response_body.text();
        }
        requests_response_body.null();

        let mut requests_response_chunks = ColumnDef::new(Requests::ResponseChunks);
        if matches!(backend, DatabaseBackend::MySql) {
            requests_response_chunks.custom(Alias::new("LONGTEXT"));
        } else {
            requests_response_chunks.text();
        }
        requests_response_chunks.null();

        let mut requests_client_ip = ColumnDef::new(Requests::ClientIp);
        requests_client_ip.text().not_null();
        if !matches!(backend, DatabaseBackend::MySql) {
            requests_client_ip.default("");
        }

        let mut requests_content_saved_at = ColumnDef::new(Requests::ContentSavedAt);
        match backend {
            DatabaseBackend::Sqlite => requests_content_saved_at.custom(Alias::new("TEXT")),
            DatabaseBackend::Postgres => requests_content_saved_at.timestamp_with_time_zone(),
            DatabaseBackend::MySql => requests_content_saved_at.timestamp(),
        };
        requests_content_saved_at.null();

        manager
            .create_table(
                Table::create()
                    .table(Requests::Table)
                    .if_not_exists()
                    .col(
                        ColumnDef::new(Requests::Id)
                            .big_integer()
                            .not_null()
                            .auto_increment()
                            .primary_key(),
                    )
                    .col(requests_created_at)
                    .col(requests_updated_at)
                    .col(ColumnDef::new(Requests::ApiKeyId).big_integer().null())
                    .col(
                        ColumnDef::new(Requests::ProjectId)
                            .big_integer()
                            .not_null()
                            .default(1),
                    )
                    .col(ColumnDef::new(Requests::TraceId).big_integer().null())
                    .col(ColumnDef::new(Requests::DataStorageId).big_integer().null())
                    .col(requests_source)
                    .col(ColumnDef::new(Requests::ModelId).text().not_null())
                    .col(requests_format)
                    .col(requests_request_headers)
                    .col(requests_request_body)
                    .col(requests_response_body)
                    .col(requests_response_chunks)
                    .col(ColumnDef::new(Requests::ChannelId).big_integer().null())
                    .col(ColumnDef::new(Requests::ExternalId).text().null())
                    .col(ColumnDef::new(Requests::Status).text().not_null())
                    .col(
                        ColumnDef::new(Requests::Stream)
                            .boolean()
                            .not_null()
                            .default(false),
                    )
                    .col(requests_client_ip)
                    .col(
                        ColumnDef::new(Requests::MetricsLatencyMs)
                            .big_integer()
                            .null(),
                    )
                    .col(
                        ColumnDef::new(Requests::MetricsFirstTokenLatencyMs)
                            .big_integer()
                            .null(),
                    )
                    .col(
                        ColumnDef::new(Requests::ContentSaved)
                            .boolean()
                            .not_null()
                            .default(false),
                    )
                    .col(
                        ColumnDef::new(Requests::ContentStorageId)
                            .big_integer()
                            .null(),
                    )
                    .col(ColumnDef::new(Requests::ContentStorageKey).text().null())
                    .col(requests_content_saved_at)
                    .foreign_key(
                        ForeignKey::create()
                            .name("fk_requests_api_key_id")
                            .from(Requests::Table, Requests::ApiKeyId)
                            .to(ApiKeys::Table, ApiKeys::Id),
                    )
                    .foreign_key(
                        ForeignKey::create()
                            .name("fk_requests_project_id")
                            .from(Requests::Table, Requests::ProjectId)
                            .to(Projects::Table, Projects::Id),
                    )
                    .foreign_key(
                        ForeignKey::create()
                            .name("fk_requests_trace_id")
                            .from(Requests::Table, Requests::TraceId)
                            .to(Traces::Table, Traces::Id),
                    )
                    .foreign_key(
                        ForeignKey::create()
                            .name("fk_requests_data_storage_id")
                            .from(Requests::Table, Requests::DataStorageId)
                            .to(DataStorages::Table, DataStorages::Id),
                    )
                    .foreign_key(
                        ForeignKey::create()
                            .name("fk_requests_channel_id")
                            .from(Requests::Table, Requests::ChannelId)
                            .to(Channels::Table, Channels::Id),
                    )
                    .to_owned(),
            )
            .await?;

        manager
            .create_index(
                Index::create()
                    .name("requests_by_api_key_id_created_at")
                    .table(Requests::Table)
                    .col(Requests::ApiKeyId)
                    .col(General::CreatedAt)
                    .if_not_exists()
                    .to_owned(),
            )
            .await?;

        manager
            .create_index(
                Index::create()
                    .name("requests_by_project_id_created_at")
                    .table(Requests::Table)
                    .col(Requests::ProjectId)
                    .col(General::CreatedAt)
                    .if_not_exists()
                    .to_owned(),
            )
            .await?;

        manager
            .create_index(
                Index::create()
                    .name("requests_by_channel_id_created_at")
                    .table(Requests::Table)
                    .col(Requests::ChannelId)
                    .col(General::CreatedAt)
                    .if_not_exists()
                    .to_owned(),
            )
            .await?;

        manager
            .create_index(
                Index::create()
                    .name("requests_by_trace_id_created_at")
                    .table(Requests::Table)
                    .col(Requests::TraceId)
                    .col(General::CreatedAt)
                    .if_not_exists()
                    .to_owned(),
            )
            .await?;

        manager
            .create_index(
                Index::create()
                    .name("requests_by_created_at")
                    .table(Requests::Table)
                    .col(General::CreatedAt)
                    .if_not_exists()
                    .to_owned(),
            )
            .await?;

        let mut request_executions_created_at = ColumnDef::new(General::CreatedAt);
        match backend {
            DatabaseBackend::Sqlite => request_executions_created_at.custom(Alias::new("TEXT")),
            DatabaseBackend::Postgres => request_executions_created_at.timestamp_with_time_zone(),
            DatabaseBackend::MySql => request_executions_created_at.timestamp(),
        };
        request_executions_created_at
            .not_null()
            .default(Expr::current_timestamp());

        let mut request_executions_updated_at = ColumnDef::new(General::UpdatedAt);
        match backend {
            DatabaseBackend::Sqlite => request_executions_updated_at.custom(Alias::new("TEXT")),
            DatabaseBackend::Postgres => request_executions_updated_at.timestamp_with_time_zone(),
            DatabaseBackend::MySql => request_executions_updated_at.timestamp(),
        };
        request_executions_updated_at
            .not_null()
            .default(Expr::current_timestamp());
        if matches!(backend, DatabaseBackend::MySql) {
            request_executions_updated_at.extra("ON UPDATE CURRENT_TIMESTAMP");
        }

        let mut request_executions_format = ColumnDef::new(RequestExecutions::Format);
        request_executions_format.text().not_null();
        if !matches!(backend, DatabaseBackend::MySql) {
            request_executions_format.default("openai/chat_completions");
        }

        let mut request_executions_request_body = ColumnDef::new(RequestExecutions::RequestBody);
        if matches!(backend, DatabaseBackend::MySql) {
            request_executions_request_body.custom(Alias::new("LONGTEXT"));
        } else {
            request_executions_request_body.text();
            request_executions_request_body.default("{}");
        }
        request_executions_request_body.not_null();

        let mut request_executions_response_body = ColumnDef::new(RequestExecutions::ResponseBody);
        if matches!(backend, DatabaseBackend::MySql) {
            request_executions_response_body.custom(Alias::new("LONGTEXT"));
        } else {
            request_executions_response_body.text();
        }
        request_executions_response_body.null();

        let mut request_executions_response_chunks =
            ColumnDef::new(RequestExecutions::ResponseChunks);
        if matches!(backend, DatabaseBackend::MySql) {
            request_executions_response_chunks.custom(Alias::new("LONGTEXT"));
        } else {
            request_executions_response_chunks.text();
        }
        request_executions_response_chunks.null();

        let mut request_executions_error_message = ColumnDef::new(RequestExecutions::ErrorMessage);
        request_executions_error_message.text().null();

        let mut request_executions_request_headers =
            ColumnDef::new(RequestExecutions::RequestHeaders);
        if matches!(backend, DatabaseBackend::MySql) {
            request_executions_request_headers.custom(Alias::new("LONGTEXT"));
        } else {
            request_executions_request_headers.text();
        }
        request_executions_request_headers.null();

        manager
            .create_table(
                Table::create()
                    .table(RequestExecutions::Table)
                    .if_not_exists()
                    .col(
                        ColumnDef::new(RequestExecutions::Id)
                            .big_integer()
                            .not_null()
                            .auto_increment()
                            .primary_key(),
                    )
                    .col(request_executions_created_at)
                    .col(request_executions_updated_at)
                    .col(
                        ColumnDef::new(RequestExecutions::ProjectId)
                            .big_integer()
                            .not_null()
                            .default(1),
                    )
                    .col(
                        ColumnDef::new(RequestExecutions::RequestId)
                            .big_integer()
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new(RequestExecutions::ChannelId)
                            .big_integer()
                            .null(),
                    )
                    .col(
                        ColumnDef::new(RequestExecutions::DataStorageId)
                            .big_integer()
                            .null(),
                    )
                    .col(ColumnDef::new(RequestExecutions::ExternalId).text().null())
                    .col(ColumnDef::new(RequestExecutions::ModelId).text().not_null())
                    .col(request_executions_format)
                    .col(request_executions_request_body)
                    .col(request_executions_response_body)
                    .col(request_executions_response_chunks)
                    .col(request_executions_error_message)
                    .col(
                        ColumnDef::new(RequestExecutions::ResponseStatusCode)
                            .big_integer()
                            .null(),
                    )
                    .col(ColumnDef::new(RequestExecutions::Status).text().not_null())
                    .col(
                        ColumnDef::new(RequestExecutions::Stream)
                            .boolean()
                            .not_null()
                            .default(false),
                    )
                    .col(
                        ColumnDef::new(RequestExecutions::MetricsLatencyMs)
                            .big_integer()
                            .null(),
                    )
                    .col(
                        ColumnDef::new(RequestExecutions::MetricsFirstTokenLatencyMs)
                            .big_integer()
                            .null(),
                    )
                    .col(request_executions_request_headers)
                    .foreign_key(
                        ForeignKey::create()
                            .name("fk_request_executions_request_id")
                            .from(RequestExecutions::Table, RequestExecutions::RequestId)
                            .to(Requests::Table, Requests::Id),
                    )
                    .foreign_key(
                        ForeignKey::create()
                            .name("fk_request_executions_channel_id")
                            .from(RequestExecutions::Table, RequestExecutions::ChannelId)
                            .to(Channels::Table, Channels::Id),
                    )
                    .foreign_key(
                        ForeignKey::create()
                            .name("fk_request_executions_data_storage_id")
                            .from(RequestExecutions::Table, RequestExecutions::DataStorageId)
                            .to(DataStorages::Table, DataStorages::Id),
                    )
                    .to_owned(),
            )
            .await?;

        manager
            .create_index(
                Index::create()
                    .name("request_executions_by_request_id_status_created_at")
                    .table(RequestExecutions::Table)
                    .col(RequestExecutions::RequestId)
                    .col(RequestExecutions::Status)
                    .col(General::CreatedAt)
                    .if_not_exists()
                    .to_owned(),
            )
            .await?;

        manager
            .create_index(
                Index::create()
                    .name("request_executions_by_channel_id_created_at")
                    .table(RequestExecutions::Table)
                    .col(RequestExecutions::ChannelId)
                    .if_not_exists()
                    .to_owned(),
            )
            .await?;

        let mut usage_logs_created_at = ColumnDef::new(General::CreatedAt);
        match backend {
            DatabaseBackend::Sqlite => usage_logs_created_at.custom(Alias::new("TEXT")),
            DatabaseBackend::Postgres => usage_logs_created_at.timestamp_with_time_zone(),
            DatabaseBackend::MySql => usage_logs_created_at.timestamp(),
        };
        usage_logs_created_at
            .not_null()
            .default(Expr::current_timestamp());

        let mut usage_logs_updated_at = ColumnDef::new(General::UpdatedAt);
        match backend {
            DatabaseBackend::Sqlite => usage_logs_updated_at.custom(Alias::new("TEXT")),
            DatabaseBackend::Postgres => usage_logs_updated_at.timestamp_with_time_zone(),
            DatabaseBackend::MySql => usage_logs_updated_at.timestamp(),
        };
        usage_logs_updated_at
            .not_null()
            .default(Expr::current_timestamp());
        if matches!(backend, DatabaseBackend::MySql) {
            usage_logs_updated_at.extra("ON UPDATE CURRENT_TIMESTAMP");
        }

        let mut usage_logs_source = ColumnDef::new(UsageLogs::Source);
        usage_logs_source.text().not_null();
        if !matches!(backend, DatabaseBackend::MySql) {
            usage_logs_source.default("api");
        }

        let mut usage_logs_format = ColumnDef::new(UsageLogs::Format);
        usage_logs_format.text().not_null();
        if !matches!(backend, DatabaseBackend::MySql) {
            usage_logs_format.default("openai/chat_completions");
        }

        let mut usage_logs_cost_items = ColumnDef::new(UsageLogs::CostItems);
        if matches!(backend, DatabaseBackend::MySql) {
            usage_logs_cost_items.custom(Alias::new("LONGTEXT"));
        } else {
            usage_logs_cost_items.text();
            usage_logs_cost_items.default("[]");
        }
        usage_logs_cost_items.not_null();

        let mut usage_logs_cost_price_reference_id =
            ColumnDef::new(UsageLogs::CostPriceReferenceId);
        usage_logs_cost_price_reference_id.text().null();

        manager
            .create_table(
                Table::create()
                    .table(UsageLogs::Table)
                    .if_not_exists()
                    .col(
                        ColumnDef::new(UsageLogs::Id)
                            .big_integer()
                            .not_null()
                            .auto_increment()
                            .primary_key(),
                    )
                    .col(usage_logs_created_at)
                    .col(usage_logs_updated_at)
                    .col(
                        ColumnDef::new(UsageLogs::RequestId)
                            .big_integer()
                            .not_null(),
                    )
                    .col(ColumnDef::new(UsageLogs::ApiKeyId).big_integer().null())
                    .col(
                        ColumnDef::new(UsageLogs::ProjectId)
                            .big_integer()
                            .not_null()
                            .default(1),
                    )
                    .col(ColumnDef::new(UsageLogs::ChannelId).big_integer().null())
                    .col(ColumnDef::new(UsageLogs::ModelId).text().not_null())
                    .col(
                        ColumnDef::new(UsageLogs::PromptTokens)
                            .big_integer()
                            .not_null()
                            .default(0),
                    )
                    .col(
                        ColumnDef::new(UsageLogs::CompletionTokens)
                            .big_integer()
                            .not_null()
                            .default(0),
                    )
                    .col(
                        ColumnDef::new(UsageLogs::TotalTokens)
                            .big_integer()
                            .not_null()
                            .default(0),
                    )
                    .col(
                        ColumnDef::new(UsageLogs::PromptAudioTokens)
                            .big_integer()
                            .not_null()
                            .default(0),
                    )
                    .col(
                        ColumnDef::new(UsageLogs::PromptCachedTokens)
                            .big_integer()
                            .not_null()
                            .default(0),
                    )
                    .col(
                        ColumnDef::new(UsageLogs::PromptWriteCachedTokens)
                            .big_integer()
                            .not_null()
                            .default(0),
                    )
                    .col(
                        ColumnDef::new(UsageLogs::PromptWriteCachedTokens5m)
                            .big_integer()
                            .not_null()
                            .default(0),
                    )
                    .col(
                        ColumnDef::new(UsageLogs::PromptWriteCachedTokens1h)
                            .big_integer()
                            .not_null()
                            .default(0),
                    )
                    .col(
                        ColumnDef::new(UsageLogs::CompletionAudioTokens)
                            .big_integer()
                            .not_null()
                            .default(0),
                    )
                    .col(
                        ColumnDef::new(UsageLogs::CompletionReasoningTokens)
                            .big_integer()
                            .not_null()
                            .default(0),
                    )
                    .col(
                        ColumnDef::new(UsageLogs::CompletionAcceptedPredictionTokens)
                            .big_integer()
                            .not_null()
                            .default(0),
                    )
                    .col(
                        ColumnDef::new(UsageLogs::CompletionRejectedPredictionTokens)
                            .big_integer()
                            .not_null()
                            .default(0),
                    )
                    .col(usage_logs_source)
                    .col(usage_logs_format)
                    .col(ColumnDef::new(UsageLogs::TotalCost).double().null())
                    .col(usage_logs_cost_items)
                    .col(usage_logs_cost_price_reference_id)
                    .foreign_key(
                        ForeignKey::create()
                            .name("fk_usage_logs_request_id")
                            .from(UsageLogs::Table, UsageLogs::RequestId)
                            .to(Requests::Table, Requests::Id),
                    )
                    .foreign_key(
                        ForeignKey::create()
                            .name("fk_usage_logs_project_id")
                            .from(UsageLogs::Table, UsageLogs::ProjectId)
                            .to(Projects::Table, Projects::Id),
                    )
                    .foreign_key(
                        ForeignKey::create()
                            .name("fk_usage_logs_channel_id")
                            .from(UsageLogs::Table, UsageLogs::ChannelId)
                            .to(Channels::Table, Channels::Id),
                    )
                    .to_owned(),
            )
            .await?;

        manager
            .create_index(
                Index::create()
                    .name("usage_logs_by_request_id")
                    .table(UsageLogs::Table)
                    .col(UsageLogs::RequestId)
                    .if_not_exists()
                    .to_owned(),
            )
            .await?;

        manager
            .create_index(
                Index::create()
                    .name("usage_logs_by_created_at")
                    .table(UsageLogs::Table)
                    .col(General::CreatedAt)
                    .if_not_exists()
                    .to_owned(),
            )
            .await?;

        manager
            .create_index(
                Index::create()
                    .name("usage_logs_by_model_id_created_at")
                    .table(UsageLogs::Table)
                    .col(UsageLogs::ModelId)
                    .col(General::CreatedAt)
                    .if_not_exists()
                    .to_owned(),
            )
            .await?;

        manager
            .create_index(
                Index::create()
                    .name("usage_logs_by_project_id_created_at")
                    .table(UsageLogs::Table)
                    .col(UsageLogs::ProjectId)
                    .col(General::CreatedAt)
                    .if_not_exists()
                    .to_owned(),
            )
            .await?;

        manager
            .create_index(
                Index::create()
                    .name("usage_logs_by_channel_id_created_at")
                    .table(UsageLogs::Table)
                    .col(UsageLogs::ChannelId)
                    .col(General::CreatedAt)
                    .if_not_exists()
                    .to_owned(),
            )
            .await?;

        manager
            .create_index(
                Index::create()
                    .name("usage_logs_by_api_key_id_created_at")
                    .table(UsageLogs::Table)
                    .col(UsageLogs::ApiKeyId)
                    .col(General::CreatedAt)
                    .if_not_exists()
                    .to_owned(),
            )
            .await?;

        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .drop_table(Table::drop().table(UsageLogs::Table).if_exists().to_owned())
            .await?;
        manager
            .drop_table(
                Table::drop()
                    .table(RequestExecutions::Table)
                    .if_exists()
                    .to_owned(),
            )
            .await?;
        manager
            .drop_table(Table::drop().table(Requests::Table).if_exists().to_owned())
            .await
    }
}

#[derive(DeriveIden)]
enum General {
    CreatedAt,
    UpdatedAt,
}

#[derive(DeriveIden)]
enum ApiKeys {
    Table,
    Id,
}

#[derive(DeriveIden)]
enum Projects {
    Table,
    Id,
}

#[derive(DeriveIden)]
enum Traces {
    Table,
    Id,
}

#[derive(DeriveIden)]
enum DataStorages {
    Table,
    Id,
}

#[derive(DeriveIden)]
enum Channels {
    Table,
    Id,
}

#[derive(DeriveIden)]
enum Requests {
    Table,
    Id,
    ApiKeyId,
    ProjectId,
    TraceId,
    DataStorageId,
    Source,
    ModelId,
    Format,
    RequestHeaders,
    RequestBody,
    ResponseBody,
    ResponseChunks,
    ChannelId,
    ExternalId,
    Status,
    Stream,
    ClientIp,
    MetricsLatencyMs,
    MetricsFirstTokenLatencyMs,
    ContentSaved,
    ContentStorageId,
    ContentStorageKey,
    ContentSavedAt,
}

#[derive(DeriveIden)]
enum RequestExecutions {
    Table,
    Id,
    ProjectId,
    RequestId,
    ChannelId,
    DataStorageId,
    ExternalId,
    ModelId,
    Format,
    RequestBody,
    ResponseBody,
    ResponseChunks,
    ErrorMessage,
    ResponseStatusCode,
    Status,
    Stream,
    MetricsLatencyMs,
    MetricsFirstTokenLatencyMs,
    RequestHeaders,
}

#[derive(DeriveIden)]
enum UsageLogs {
    Table,
    Id,
    RequestId,
    ApiKeyId,
    ProjectId,
    ChannelId,
    ModelId,
    PromptTokens,
    CompletionTokens,
    TotalTokens,
    PromptAudioTokens,
    PromptCachedTokens,
    PromptWriteCachedTokens,
    #[sea_orm(iden = "prompt_write_cached_tokens_5m")]
    PromptWriteCachedTokens5m,
    #[sea_orm(iden = "prompt_write_cached_tokens_1h")]
    PromptWriteCachedTokens1h,
    CompletionAudioTokens,
    CompletionReasoningTokens,
    CompletionAcceptedPredictionTokens,
    CompletionRejectedPredictionTokens,
    Source,
    Format,
    TotalCost,
    CostItems,
    CostPriceReferenceId,
}
