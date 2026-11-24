# Privacidade e dados locais

Este documento descreve os dados que o Rustify usa hoje. A sincronização OAuth é opcional; importar links públicos ou arquivos exportados não exige conectar uma conta.

## Dados acessados

Ao conectar uma conta, o Rustify solicita estes scopes OAuth:

- `user-read-private`
- `user-library-read`
- `playlist-read-private`
- `playlist-read-collaborative`

O código sincroniza músicas salvas e playlists da conta, incluindo os metadados e as faixas de cada playlist. Ele não consulta o endpoint de perfil. Links públicos de playlists, álbuns ou faixas podem ser importados sem OAuth; nesse caso o app lê a página de embed pública do Spotify.

O Rustify guarda título, artistas, álbum, duração, identidade e URL da capa. Ao reproduzir uma faixa importada, título e artista são usados numa busca `yt-dlp` no YouTube. O player só solicita o áudio quando você inicia a reprodução; o libmpv recebe o stream e pode tocar enquanto preenche seu buffer. Curtidas e importação de playlists do YouTube guardam metadados, sem baixar previamente os áudios. O Rustify não recebe áudio do Spotify.

Arquivos JSON/CSV escolhidos pelo usuário são lidos localmente e convertidos em dados da biblioteca. O Rustify não altera esses arquivos de origem.

## Verificação de atualização

Por padrão, ao abrir o app (no máximo uma vez a cada 24 horas) e quando você pede em **Sobre e atualizações** (versão no rodapé da barra lateral ou botão na Biblioteca), o Rustify faz um `GET` público em `https://api.github.com/repos/1arley/rustify/releases/latest` com o cabeçalho `User-Agent: rustify/<versão>`. Nenhum dado seu, identificador ou telemetria é enviado, e nada é baixado ou instalado: o app só compara versões e, se você pedir, abre a página da release no navegador. Falhas de rede são ignoradas. Desligue a opção **Verificar atualizações ao abrir** na mesma tela para não fazer essa consulta automática.

## Armazenamento

As pastas seguem as convenções de cada sistema operacional por meio de `dirs`:

- Configuração: `dirs::config_dir()/rustify`.
- Biblioteca: `dirs::data_local_dir()/rustify`.
- Linux respeita `XDG_CONFIG_HOME` e `XDG_DATA_HOME`; Windows usa as pastas de configuração e dados locais do usuário; macOS usa Application Support.

Arquivos usados:

- `client_id` e `token.json` na pasta de configuração. O token contém credenciais OAuth e é removido ao desconectar o Client ID configurado.
- `spotify-account.json` na pasta de dados. Guarda o último snapshot completo da conta e as correspondências YouTube pertencentes a esse snapshot.
- `liked-tracks.json`, `playlists.json` e `resolved-tracks.json` na pasta de dados. Guardam a biblioteca local, playlists criadas ou importadas e suas correspondências.
- `update-check.json` na pasta de dados. Guarda `enabled` (verificação ligada ou desligada), `last_checked_at`, `dismissed_version` e `latest_seen` para o aviso de nova versão.
- `playback-session.json` na pasta de dados. Guarda a última faixa e posição para restaurar o player pausado na próxima abertura; a fila não é persistida.

Os dados ficam neste dispositivo e não expiram automaticamente. Uma sincronização substitui o snapshot da conta só depois de terminar. Uma falha mantém o snapshot anterior.

## Desconexão e remoção

Desconectar remove o snapshot da conta e o token OAuth para o Client ID ativo; curtidas e playlists locais são preservadas. Os dados locais de versões antigas podem ter misturado itens da conta e imports manuais sem registrar a origem. O Rustify preserva esses itens ambíguos por padrão.

Na tela **Importar do Spotify**, a ação **Remover imports antigos ligados ao Spotify** pede confirmação e apaga somente faixas com ID ou link Spotify reconhecível e playlists com ID ou URL Spotify reconhecível nos arquivos locais. Itens cuja origem não pode ser identificada permanecem. Essa limpeza não pode ser desfeita.
