{
    const sessionKey = 'tauritavern:dev-sw-session';
    const isNewSession = sessionStorage.getItem(sessionKey) === null;
    sessionStorage.setItem(sessionKey, 'active');

    // Registrations outlive the process-local Wry protocol handler. Rebind only
    // when a new WebView session inherits the previous process's controller.
    if (isNewSession && navigator.serviceWorker?.controller) {
        window.stop();
        void navigator.serviceWorker.getRegistrations()
            .then((registrations) => Promise.all(
                registrations.map((registration) => registration.unregister()),
            ))
            .then(() => window.location.reload());
    }
}
