const app = new TauriTavern();
app.on('notification', (event) => {
  if (event.type === 'click' && event.data.message === '點擊返回 TauriTavern') {
    // Move to top of window after click
    app.window.moveToTop();
  }
});