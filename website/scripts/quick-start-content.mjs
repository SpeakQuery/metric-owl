export default `
<span class="eyebrow">QUICK START · FREE PUBLIC PREVIEW</span>
<h1>Install once.<br>Open anywhere.</h1>
<p class="lead">Three steps to your own server dashboard. No app to install on the computer or phone you view it from.</p>
<p class="quick-requirements">You need Ubuntu 24.04 x86-64 and a dedicated domain or subdomain pointing to that server.</p>
<ol class="quick-steps">
<li><span class="quick-number">01</span><div><h2>Run one command on your server.</h2><p>Paste this into your server’s terminal and press Enter.</p><div class="connection-command quick-install-command"><div><strong>Install Metric Owl</strong><button type="button" data-copy-value="curl -fsSL https://metricowl.cloud/install.sh | sudo bash">Copy command</button></div><pre><code>curl -fsSL https://metricowl.cloud/install.sh | sudo bash</code></pre></div></div></li>
<li><span class="quick-number">02</span><div><h2>Answer the installer.</h2><p>Enter your panel domain, choose an administrator username and password, then follow the HTTPS prompts. The installer prints your panel address when it finishes.</p><p class="quick-example">For example: <code>https://monitor.example.com/admin</code></p></div></li>
<li><span class="quick-number">03</span><div><h2>Open your panel and sign in.</h2><p>Open the printed address in any browser. Sign in with the username and password you just chose. That’s it — your live dashboard is ready.</p><p>No setup key or authenticator app is required. You can add optional two-factor authentication later in Security &amp; system.</p></div></li>
</ol>
<div class="quick-finish"><strong>Your server. Your dashboard.</strong><p>The address belongs to your own installation. You do not need a Metric Owl cloud account or an AI key to start monitoring.</p></div>
<p class="quick-help" id="mcp">Need SSH, MCP, manual setup or an existing web-server configuration? <a href="/docs/advanced.html">Open the advanced guide →</a></p>
<p class="quick-help"><a href="https://github.com/SpeakQuery/metric-owl/blob/v0.1.0-preview.3/scripts/bootstrap-linux.sh">Inspect the installer on GitHub</a> · <a href="mailto:services@metricowl.cloud">Get help</a></p>
`;
