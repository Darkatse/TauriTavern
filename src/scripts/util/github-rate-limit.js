export const GITHUB_RATE_LIMIT_MESSAGE =
    'GitHub has rate-limited your requests. Please try again later, or change your network and try again.';

export function isGitHubRateLimitMessage(value) {
    return String(value || '').trim() === GITHUB_RATE_LIMIT_MESSAGE;
}

export function isGitHubRateLimitStatus(status) {
    return Number(status) === 429;
}

