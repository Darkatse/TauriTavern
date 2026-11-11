// Injected only by tauri-dev-server, before the application's module scripts.
(() => {
    // Mobile dev proxies this document through the app protocol, which serves these resources natively.
    if (location.origin !== document.currentScript.dataset.devServerOrigin) return;

    const serviceWorker = navigator.serviceWorker;
    if (!serviceWorker) {
        console.warn('Tauri dev resource bridge is unavailable: Service Workers are not supported in this context. User CSS, third-party extensions and user resources will not be available through the dev server.');
        return;
    }

    const sessionKey = 'tauritavern:dev-sw-session';
    const isNewSession = sessionStorage.getItem(sessionKey) === null;
    sessionStorage.setItem(sessionKey, 'active');

    // Registrations outlive the process-local Wry protocol handler.
    if (isNewSession && serviceWorker.controller) {
        window.stop();
        void rebind();
        return;
    }

    const bridge = import('./tauri-bridge.js');
    const baseUrl = bridge.then(({ convertFileSrc }) => convertFileSrc('', 'tt-ext'));
    serviceWorker.addEventListener('message', ({ data, ports: [port] }) => {
        if (data?.type === 'tt-ext-proxy-request') void proxy(data, port);
    });
    // Head stylesheets may load through this proxy while module scripts wait for them.
    serviceWorker.startMessages();
    window.__TAURITAVERN_DEV_SW_READY__ = install();

    async function rebind() {
        const registrations = await serviceWorker.getRegistrations();
        await Promise.all(registrations.map(registration => registration.unregister()));
        window.location.reload();
    }

    async function proxy({ pathname, search, method, headers, cache, requiresIpc }, port) {
        try {
            let response;
            let body;
            if (requiresIpc) {
                const { invoke } = await bridge;
                response = await invoke('read_dev_web_resource', {
                    request: { pathname, search, method, headers },
                });
                body = Uint8Array.from(response.body).buffer;
            } else {
                response = await fetch(new URL(`${pathname}${search}`, await baseUrl), {
                    method, headers, cache, credentials: 'omit',
                });
                body = await response.arrayBuffer();
            }
            port.postMessage({
                ok: true,
                status: response.status,
                statusText: response.statusText,
                headers: requiresIpc ? response.headers : Array.from(response.headers.entries()),
                body,
            }, [body]);
        } catch (error) {
            port.postMessage({ ok: false, error: String(error?.message || error) });
        }
    }

    async function install() {
        await serviceWorker.register(`/tt-ext-sw.js?base=${encodeURIComponent(await baseUrl)}`, { scope: '/' });
        const registration = await serviceWorker.ready;
        if (!serviceWorker.controller) {
            // A force reload stays uncontrolled without a new activation.
            await new Promise(resolve => {
                serviceWorker.addEventListener('controllerchange', resolve, { once: true });
                registration.active.postMessage({ type: 'tt-ext-claim' });
            });
        }
        const announce = () => serviceWorker.controller?.postMessage({ type: 'tt-ext-proxy-ready' });
        announce();
        serviceWorker.addEventListener('controllerchange', announce);
    }
})();
