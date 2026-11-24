// Helpers for JSON responses and request bodies
export function send(res, code, obj) {
  res.status(code).json(obj);
}

export function body(req) {
  if (!req.body) return {};
  if (typeof req.body === "string") {
    try {
      return JSON.parse(req.body);
    } catch {
      return {};
    }
  }
  return req.body;
}

export function methodOnly(req, res, method) {
  if (req.method !== method) {
    res.setHeader("Allow", method);
    send(res, 405, { error: "method not allowed" });
    return false;
  }
  return true;
}
