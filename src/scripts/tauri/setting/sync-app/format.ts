export function formatTimestampValue(
    ms: number | null | undefined,
    tr: (key: string) => string,
): string {
    if (!ms) {
        return tr('N/A');
    }

    const date = new Date(Number(ms));
    if (Number.isNaN(date.getTime())) {
        return tr('Invalid time');
    }

    return date.toLocaleString();
}
