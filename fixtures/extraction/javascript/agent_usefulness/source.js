const attrs = { 'hx-post': '/save', 'hx-trigger': 'click', 'hx-get': `/workspaces/${id}/tests/start` };
const logging = { 'hx-get': '/not-consumed' };
export const View = () => <button {...attrs}>Save</button>;
