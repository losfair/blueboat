export function serveStaticFiles(stripPrefix: string, addPrefix: string): (req: Request) => Promise<Response> {
  if (!stripPrefix.startsWith('/')) {
    stripPrefix = '/' + stripPrefix;
  }
  if (!stripPrefix.endsWith('/')) {
    stripPrefix = stripPrefix + '/';
  }

  if (addPrefix.startsWith('/')) {
    addPrefix = addPrefix.substring(1);
  }
  if (!addPrefix.endsWith('/')) {
    addPrefix = addPrefix + '/';
  }

  const notFound = () => new Response("Page not found.", { status: 404, headers: { 'Content-Type': 'text/plain' } });

  return async req => {
    const u = new URL(req.url);
    if (!u.pathname.startsWith(stripPrefix)) {
      return notFound();
    }
    const path = u.pathname.substr(stripPrefix.length);
    let packagePath = addPrefix + path;
    if (packagePath.endsWith('/')) {
      packagePath += 'index.html';
    }

    let file = Package[packagePath];
    if (!file) {
      return notFound();
    }

    const entityTag = computeEntityTag(file);
    if (req.headers.get("if-none-match") === entityTag) {
      return new Response(null, { status: 304 });
    }

    const fileExt = extractFileExt(packagePath);
    const contentType = fileExt && Dataset.Mime.guessByExt(fileExt);

    return new Response(file, {
      status: 200,
      headers: {
        'Content-Type': contentType || "text/plain",
        'ETag': entityTag,
      }
    });
  }
}

function computeEntityTag(bytes: Uint8Array): string {
  const hash = Codec.b64encode(NativeCrypto.digest('sha1', bytes)).substring(0, 27);
  return '"' + bytes.length.toString(16) + '-' + hash + '"'
}

function extractFileExt(path: string): string {
  const parts = path.split('/').pop()!.split('.');
  if (parts.length > 1) {
    return parts[parts.length - 1];
  }
  return '';
}