import { translate } from '../i18n.js';
import { GITHUB_RATE_LIMIT_MESSAGE } from './github-rate-limit.js';

export class GitHubRateLimitStopper {
    #tripped = false;
    #notified = false;

    isTripped() {
        return this.#tripped;
    }

    trip() {
        this.#tripped = true;
        if (this.#notified) {
            return false;
        }

        this.#notified = true;
        toastr.warning(translate(GITHUB_RATE_LIMIT_MESSAGE));
        return true;
    }
}

export const githubRateLimitStopper = new GitHubRateLimitStopper();

