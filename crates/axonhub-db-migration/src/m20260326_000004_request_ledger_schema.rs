use sea_orm_migration::prelude::*;

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .create_table(
                Table::create()
                    .table(Requests::Table)
                    .if_not_exists()
                    .col(primary_id_column(Requests::Id))
                    .col(timestamp_column(General::CreatedAt))
                    .col(timestamp_column(General::UpdatedAt))
                    .col(ColumnDef::new(Requests::ApiKeyId).big_integer().null())
                    .col(
                        ColumnDef::new(Requests::ProjectId)
                            .big_integer()
                            .not_null()
                            .default(1),
                    )
                    .col(ColumnDef::new(Requests::TraceId).big_integer().null())
                    .col(ColumnDef::new(Requests::DataStorageId).big_integer().null())
                    .col(
                        ColumnDef::new(Requests::Source)
                            .text()
                            .not_null()
                            .default("api"),
                    )
                    .col(ColumnDef::new(Requests::ModelId).text().not_null())
                    .col(
                        ColumnDef::new(Requests::Format)
                            .text()
                            .not_null()
                            .default("openai/chat_completions"),
                    )
                    .col(ColumnDef::new(Requests::RequestHeaders).text().null())
                    .col(
                        ColumnDef::new(Requests::RequestBody)
                            .text()
                            .not_null()
                            .default("{}"),
                    )
                    .col(ColumnDef::new(Requests::ResponseBody).text().null())
                    .col(ColumnDef::new(Requests::ResponseChunks).text().null())
                    .col(ColumnDef::new(Requests::ChannelId).big_integer().null())
                    .col(ColumnDef::new(Requests::ExternalId).text().null())
                    .col(ColumnDef::new(Requests::Status).text().not_null())
                    .col(
                        ColumnDef::new(Requests::Stream)
                            .boolean()
                            .not_null()
                            .default(false),
                    )
                    .col(
                        ColumnDef::new(Requests::ClientIp)
                            .text()
                            .not_null()
                            .default(""),
                    )
                    .col(ColumnDef::new(Requests::MetricsLatencyMs).big_integer().null())
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
                    .col(ColumnDef::new(Requests::ContentStorageId).big_integer().null())
                    .col(ColumnDef::new(Requests::ContentStorageKey).text().null())
                    .col(nullable_text_timestamp_column(Requests::ContentSavedAt))
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

        manager
            .create_table(
                Table::create()
                    .table(RequestExecutions::Table)
                    .if_not_exists()
                    .col(primary_id_column(RequestExecutions::Id))
                    .col(timestamp_column(General::CreatedAt))
                    .col(timestamp_column(General::UpdatedAt))
                    .col(
                        ColumnDef::new(RequestExecutions::ProjectId)
                            .big_integer()
                            .not_null()
                            .default(1),
                    )
                    .col(ColumnDef::new(RequestExecutions::RequestId).big_integer().not_null())
                    .col(ColumnDef::new(RequestExecutions::ChannelId).big_integer().null())
                    .col(ColumnDef::new(RequestExecutions::DataStorageId).big_integer().null())
                    .col(ColumnDef::new(RequestExecutions::ExternalId).text().null())
                    .col(ColumnDef::new(RequestExecutions::ModelId).text().not_null())
                    .col(
                        ColumnDef::new(RequestExecutions::Format)
                            .text()
                            .not_null()
                            .default("openai/chat_completions"),
                    )
                    .col(
                        ColumnDef::new(RequestExecutions::RequestBody)
                            .text()
                            .not_null()
                            .default("{}"),
                    )
                    .col(ColumnDef::new(RequestExecutions::ResponseBody).text().null())
                    .col(ColumnDef::new(RequestExecutions::ResponseChunks).text().null())
                    .col(ColumnDef::new(RequestExecutions::ErrorMessage).text().null())
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
                    .col(ColumnDef::new(RequestExecutions::RequestHeaders).text().null())
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

        manager
            .create_table(
                Table::create()
                    .table(UsageLogs::Table)
                    .if_not_exists()
                    .col(primary_id_column(UsageLogs::Id))
                    .col(timestamp_column(General::CreatedAt))
                    .col(timestamp_column(General::UpdatedAt))
                    .col(ColumnDef::new(UsageLogs::RequestId).big_integer().not_null())
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
                    .col(
                        ColumnDef::new(UsageLogs::Source)
                            .text()
                            .not_null()
                            .default("api"),
                    )
                    .col(
                        ColumnDef::new(UsageLogs::Format)
                            .text()
                            .not_null()
                            .default("openai/chat_completions"),
                    )
                    .col(ColumnDef::new(UsageLogs::TotalCost).double().null())
                    .col(
                        ColumnDef::new(UsageLogs::CostItems)
                            .text()
                            .not_null()
                            .default("[]"),
                    )
                    .col(ColumnDef::new(UsageLogs::CostPriceReferenceId).text().null())
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

fn primary_id_column(iden: impl IntoIden) -> ColumnDef {
    let mut column = ColumnDef::new(iden);
    column.big_integer().not_null().auto_increment().primary_key();
    column
}

fn timestamp_column(iden: impl IntoIden) -> ColumnDef {
    let mut column = ColumnDef::new(iden);
    column
        .custom(Alias::new("TEXT"))
        .not_null()
        .default(Expr::cust("CURRENT_TIMESTAMP::text"));
    column
}

fn nullable_text_timestamp_column(iden: impl IntoIden) -> ColumnDef {
    let mut column = ColumnDef::new(iden);
    column.custom(Alias::new("TEXT")).null();
    column
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
