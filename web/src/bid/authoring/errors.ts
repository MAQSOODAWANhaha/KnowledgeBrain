export class AuthoringLogicError extends Error {
  readonly code: string;
  readonly technical: boolean;
  readonly requestArtifactId?: string;

  constructor(
    code: string,
    message: string,
    technical = true,
    requestArtifactId?: string,
  ) {
    super(message);
    this.name = "AuthoringLogicError";
    this.code = code;
    this.technical = technical;
    this.requestArtifactId = requestArtifactId;
  }
}

export class EditorAdapterError extends AuthoringLogicError {
  constructor(message: string) {
    super("EDITOR_ADAPTER", message, true);
    this.name = "EditorAdapterError";
  }
}

export class CasConflictError extends AuthoringLogicError {
  constructor(
    message = "工作区已由他人更新，请选择保留本地草稿或使用服务器版本。",
  ) {
    super("CAS_CONFLICT", message, true);
    this.name = "CasConflictError";
  }
}

export class EnqueueUncertainError extends AuthoringLogicError {
  constructor(requestArtifactId?: string) {
    super(
      "ENQUEUE_UNCERTAIN",
      "请求已提交但排队未确认，将用同一幂等键重试。",
      true,
      requestArtifactId,
    );
    this.name = "EnqueueUncertainError";
  }
}
