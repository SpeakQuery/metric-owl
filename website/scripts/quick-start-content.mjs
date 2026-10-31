export default `
<span class="eyebrow">QUICK START · FREE PUBLIC PREVIEW</span>
<h1>Install once.<br>Open anywhere.</h1>
<p class="lead">Three steps to your own server dashboard. No app to install on the computer or phone you view it from.</p>
<p class="quick-requirements">You need Ubuntu or Debian x86-64 with a public IPv4 address. No domain or cloud account required. <a href="/docs/releases.html#platform-status">Versions and validation →</a></p>
<ol class="quick-steps">
<li><span class="quick-number">01</span><div><h2>Run one command on your server.</h2><p>Paste this into your server’s terminal. The installer and application package come directly from GitHub. Your OS is detected locally; no machine inventory is uploaded.</p><div class="connection-command quick-install-command"><div><strong>Install Metric Owl</strong><button type="button" data-copy-value="curl -fsSL https://raw.githubusercontent.com/SpeakQuery/metric-owl/v0.1.0-preview.5/scripts/bootstrap-linux.sh | sudo bash">Copy command</button></div><pre><code>curl -fsSL https://raw.githubusercontent.com/SpeakQuery/metric-owl/v0.1.0-preview.5/scripts/bootstrap-linux.sh | sudo bash</code></pre></div></div></li>
<li><span class="quick-number">02</span><div><h2>Answer the installer.</h2><p>Confirm the detected public IP, review the Let’s Encrypt terms and enter <code>yes</code> to accept. Then choose an administrator username and enter your password twice. The installer prints your panel address when it finishes.</p><p class="quick-example">For example: <code>https://YOUR_SERVER_IP:PORT/admin</code></p></div></li>
<li><span class="quick-number">03</span><div><h2>Open your panel and sign in.</h2><p>Open the printed address in any browser. Sign in with the username and password you just chose. That’s it — your live dashboard is ready.</p><p>No setup key or authenticator app is required. You can add optional two-factor authentication later in Security &amp; system.</p></div></li>
</ol>
<div class="quick-finish"><strong>Your server. Your dashboard.</strong><p>Your application, dashboard, account, metrics and SQLite history run on your own server. No central cloud account is needed. Optional AI, email, MCP and webhooks share selected data only when you enable them.</p></div>
<p class="quick-help">The installer selects port 8443, or 9443 / 10443 if occupied, and prints the exact URL. Port 80 for Metric Owl certificate validation and the selected panel port must be reachable. <a href="/docs/advanced.html#network-access">Network details →</a></p>
<p class="quick-help"><strong>Uninstall:</strong> <code>sudo bash /usr/share/doc/server-guard/uninstall-linux.sh</code> — your account and history are retained. <a href="/docs/advanced.html#uninstall">Removal details →</a></p>
<p class="quick-help" id="mcp">Need SSH, MCP, manual setup or an existing web-server configuration? <a href="/docs/advanced.html">Open the advanced guide →</a></p>
<p class="quick-help"><a href="https://github.com/SpeakQuery/metric-owl/blob/v0.1.0-preview.5/scripts/bootstrap-linux.sh">Inspect the installer on GitHub</a> · <a href="mailto:services@metricowl.cloud">Get help</a></p>
`;
