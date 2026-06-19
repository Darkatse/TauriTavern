export function mapChatSummaryResults(context, results) {
    return Array.isArray(results)
        ? results.map((entry) => ({
            // SillyTavern's /api/chats/search exposes the extensionless file id as file_name.
            file_name: context.stripJsonl(entry.file_name),
            file_size: context.formatFileSize(entry.file_size),
            message_count: Number(entry.message_count || 0),
            preview_message: entry.preview || '',
            last_mes: Number(entry.date || 0),
        }))
        : [];
}
