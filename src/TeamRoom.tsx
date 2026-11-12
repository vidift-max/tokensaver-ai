export default function TeamRoom() {
  return (
    <section className="team-room" aria-labelledby="team-room-security-title">
      <div className="panel">
        <h2 id="team-room-security-title">AI Team Room — acesso desktop suspenso</h2>
        <p>
          A conexão remota do Team Room pelo aplicativo desktop está temporariamente
          desabilitada por segurança.
        </p>
        <p>
          Esta versão ainda não possui login OIDC Authorization Code + PKCE integrado
          ao desktop nem armazenamento de sessão no cofre seguro do sistema operacional.
          Por isso, o aplicativo não aceita endpoint remoto nem bearer token digitado
          manualmente.
        </p>
        <p>
          O backend Team Room e os testes de Runner continuam separados. O acesso desktop
          só será reativado após autenticação nativa, allowlist de origem HTTPS, logout e
          revogação integrados e testes de segurança no WebView.
        </p>
      </div>
    </section>
  );
}
