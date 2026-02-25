import { renderExtensionTemplateAsync } from '../../extensions.js';
import { callGenericPopup, POPUP_TYPE } from '../../popup.js';

const MODULE_NAME = 'lan-sync';
const UPDATE_INTERVAL = 3000;

let isServerRunning = false;
let lastAddress = '';

async function updateStatus() {
    try {
        const status = await window.__TAURI__.core.invoke('plugin_lan_sync_status');
        isServerRunning = status.is_running;
        
        const statusText = $('#lan-sync-status-text');
        const toggleBtn = $('#lan-sync-toggle-btn');
        const infoBox = $('#lan-sync-info-box');
        const addressText = $('#lan-sync-address-text');

        if (isServerRunning) {
            statusText.text('正在运行').css('color', '#0f0');
            toggleBtn.text('停止服务');
            infoBox.show();
            const address = status.address || 'N/A';
            addressText.text(address);
            
            // 如果地址发生变化，重新生成二维码
            if (address !== 'N/A' && address !== lastAddress) {
                lastAddress = address;
                generateQRCode(address);
            }
        } else {
            statusText.text('已停止').css('color', '#f00');
            toggleBtn.text('启动服务');
            infoBox.hide();
            lastAddress = '';
        }
    } catch (e) {
        console.error('[LAN Sync] 更新状态失败:', e);
    }
}

async function generateQRCode(text) {
    const qrImg = $('#lan-sync-qr-img');
    const qrLoading = $('#lan-sync-qr-loading');
    
    qrImg.hide();
    qrLoading.show();
    
    try {
        const svg = await window.__TAURI__.core.invoke('plugin_lan_sync_qr', { text });
        // 将 SVG 转换为 Data URL
        const blob = new Blob([svg], { type: 'image/svg+xml' });
        const url = URL.createObjectURL(blob);
        qrImg.attr('src', url).show();
    } catch (e) {
        console.error('[LAN Sync] 生成二维码失败:', e);
    } finally {
        qrLoading.hide();
    }
}

async function toggleServer() {
    try {
        if (isServerRunning) {
            await window.__TAURI__.core.invoke('plugin_lan_sync_stop');
        } else {
            await window.__TAURI__.core.invoke('plugin_lan_sync_start');
        }
        await updateStatus();
    } catch (e) {
        callGenericPopup(`操作失败: ${e}`, POPUP_TYPE.TEXT);
    }
}

jQuery(async () => {
    const container = $('#lan_sync_container');
    if (!container.length) return;

    const html = await renderExtensionTemplateAsync(MODULE_NAME, 'settings');
    container.append(html);

    container.find('#lan-sync-toggle-btn').on('click', toggleServer);
    
    // 初始化状态
    await updateStatus();
    
    // 开启定时更新
    setInterval(updateStatus, UPDATE_INTERVAL);

    console.log('[LAN Sync] 插件已就绪');
});
