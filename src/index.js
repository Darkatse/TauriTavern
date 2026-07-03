window.addEventListener('message', (event) => { // Add event listener for message events
if (event.data.type === 'TauriTavern') {
  if (event.data.action === 'close') {
    TauriTavern.close();
  }
}
}, window);
// Add event listener for notification messages
window.addEventListener('message', (event) => { // Add event listener for message events
if (event.data.type === 'TauriTavern') {
  if (event.data.action === 'close') {
    TauriTavern.close();
  }
}
});