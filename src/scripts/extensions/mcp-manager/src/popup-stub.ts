/**
 * Test double for the SillyTavern Popup host API: records the instance,
 * appends content to the document like the real popup, and lets tests drive
 * closing through the same onClosing veto semantics.
 */
export type TestPopupOptions = {
    onOpen?: () => void;
    onClosing?: (popup: TestPopup) => boolean | Promise<boolean>;
};

export class TestPopup {
    static current: TestPopup | undefined;

    result: unknown;
    private resolve: ((value: unknown) => void) | undefined;

    constructor(
        readonly content: Element,
        type: number,
        inputValue: string,
        private readonly options: TestPopupOptions,
    ) {
        void type;
        void inputValue;
        TestPopup.current = this;
    }

    show(): Promise<unknown> {
        document.body.append(this.content);
        setTimeout(() => this.options.onOpen?.());
        return new Promise(resolve => {
            this.resolve = resolve;
        });
    }

    async close(result: unknown): Promise<boolean> {
        this.result = result;
        if (this.options.onClosing && !await this.options.onClosing(this)) {
            return false;
        }
        this.content.remove();
        this.resolve?.(result);
        return true;
    }
}

export function installPopupHost(): void {
    Object.defineProperty(window, 'SillyTavern', {
        configurable: true,
        value: {
            getContext: () => ({
                Popup: TestPopup,
                POPUP_TYPE: { TEXT: 1, CONFIRM: 2, INPUT: 3 },
                POPUP_RESULT: { AFFIRMATIVE: 1 },
            }),
        },
    });
}

export function uninstallPopupHost(): void {
    TestPopup.current?.content.remove();
    TestPopup.current = undefined;
    Reflect.deleteProperty(window, 'SillyTavern');
}
