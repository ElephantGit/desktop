import { useId, useRef, useState, type RefObject } from "react";
import { useTranslation } from "react-i18next";
import {
  Button,
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
  DialogTrigger,
  Input,
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  Switch,
  toast,
} from "@ora/ui";
import { IconLoader2, IconPlus } from "@tabler/icons-react";
import type {
  MarketplaceArtifactRetrieval,
  MarketplaceArtifactRetrievalUpdate,
} from "@ora/contracts";
import { localizeContractError } from "../../i18n/contract-error";
import { useContractErrorToast } from "../../i18n/use-contract-error-toast";
import { useAddMarketplaceSource } from "../../state/hooks/use-marketplace-sources";

/** Owns the new-source draft while the existing mutations own persistence. */
export function PluginSourceAddDialog() {
  const { t } = useTranslation();
  const showContractError = useContractErrorToast();
  const addSource = useAddMarketplaceSource();
  const [open, setOpen] = useState(false);
  const [url, setUrl] = useState("");
  const [branch, setBranch] = useState("main");
  const [useProxy, setUseProxy] = useState(false);
  const [retrievalType, setRetrievalType] =
    useState<MarketplaceArtifactRetrieval["type"]>("direct_https");
  const [endpoint, setEndpoint] = useState("");
  const [bucket, setBucket] = useState("");
  const [region, setRegion] = useState("");
  const [accessKeyId, setAccessKeyId] = useState("");
  const [secretAccessKey, setSecretAccessKey] = useState("");
  const [submitted, setSubmitted] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const urlInput = useRef<HTMLInputElement>(null);
  const branchInput = useRef<HTMLInputElement>(null);
  const endpointInput = useRef<HTMLInputElement>(null);
  const bucketInput = useRef<HTMLInputElement>(null);
  const regionInput = useRef<HTMLInputElement>(null);
  const accessKeyInput = useRef<HTMLInputElement>(null);
  const secretKeyInput = useRef<HTMLInputElement>(null);
  const submitting = useRef(false);
  const id = useId();
  const urlMissing = submitted && url.trim() === "";
  const branchMissing = submitted && branch.trim() === "";
  const s3Selected = retrievalType === "s3_sigv4";
  const endpointMissing = submitted && s3Selected && endpoint.trim() === "";
  const bucketMissing = submitted && s3Selected && bucket.trim() === "";
  const regionMissing = submitted && s3Selected && region.trim() === "";
  const accessKeyMissing = submitted && s3Selected && accessKeyId.trim() === "";
  const secretKeyMissing =
    submitted && s3Selected && secretAccessKey.trim() === "";
  const s3Incomplete =
    endpointMissing ||
    bucketMissing ||
    regionMissing ||
    accessKeyMissing ||
    secretKeyMissing;

  const resetDraft = () => {
    setUrl("");
    setBranch("main");
    setUseProxy(false);
    setRetrievalType("direct_https");
    setEndpoint("");
    setBucket("");
    setRegion("");
    setAccessKeyId("");
    setSecretAccessKey("");
    setSubmitted(false);
    setError(null);
    addSource.reset();
  };

  const artifactRetrieval = (): MarketplaceArtifactRetrievalUpdate => {
    if (!s3Selected) return { type: "direct_https" };
    return {
      type: "s3_sigv4",
      endpoint: endpoint.trim(),
      bucket: bucket.trim(),
      region: region.trim(),
      credentials: {
        action: "replace",
        accessKeyId: accessKeyId.trim(),
        secretAccessKey: secretAccessKey.trim(),
      },
    };
  };

  return (
    <Dialog
      open={open}
      onOpenChange={(nextOpen, details) => {
        if (submitting.current) {
          details.cancel();
          return;
        }
        if (nextOpen) resetDraft();
        setOpen(nextOpen);
      }}
    >
      <DialogTrigger render={<Button size="sm" />}>
        <IconPlus />
        {t("settings.plugins.addSource")}
      </DialogTrigger>
      <DialogContent
        className="gap-0 overflow-hidden p-0 sm:max-w-md"
        initialFocus={urlInput}
        showCloseButton={!addSource.isPending}
      >
        <form
          className="flex flex-col"
          noValidate
          aria-busy={addSource.isPending}
          onSubmit={(event) => {
            event.preventDefault();
            if (submitting.current) return;
            setSubmitted(true);
            setError(null);
            if (url.trim() === "") {
              urlInput.current?.focus();
              return;
            }
            if (branch.trim() === "") {
              branchInput.current?.focus();
              return;
            }
            if (s3Selected) {
              const firstInvalid = (
                [
                  [endpoint.trim() === "", endpointInput],
                  [bucket.trim() === "", bucketInput],
                  [region.trim() === "", regionInput],
                  [accessKeyId.trim() === "", accessKeyInput],
                  [secretAccessKey.trim() === "", secretKeyInput],
                ] as const
              ).find(([missing]) => missing)?.[1];
              if (firstInvalid !== undefined) {
                firstInvalid.current?.focus();
                return;
              }
            }
            submitting.current = true;
            addSource.mutate(
              {
                url: url.trim(),
                branch: branch.trim(),
                useProxy,
                artifactRetrieval: artifactRetrieval(),
              },
              {
                onSuccess: () => {
                  setOpen(false);
                  toast.success(t("settings.plugins.sourceAdded"));
                },
                onError: (cause) => {
                  setError(localizeContractError(cause, t));
                  showContractError(
                    cause,
                    t("settings.plugins.sourceAddFailed"),
                  );
                },
                onSettled: () => {
                  submitting.current = false;
                },
              },
            );
          }}
        >
          <div className="max-h-[calc(min(40rem,100dvh-2rem)-4.5rem)] space-y-4 overflow-y-auto p-4">
            <DialogHeader className="pr-8">
              <DialogTitle>{t("settings.plugins.addSource")}</DialogTitle>
              <DialogDescription>
                {t("settings.plugins.addSourceDescription")}
              </DialogDescription>
            </DialogHeader>
            <div className="space-y-1.5">
              <label htmlFor={`${id}-url`} className="text-sm font-medium">
                {t("settings.plugins.sourceUrl")}
              </label>
              <Input
                ref={urlInput}
                id={`${id}-url`}
                value={url}
                onChange={(event) => setUrl(event.target.value)}
                placeholder="https://github.com/owner/marketplace"
                autoComplete="off"
                spellCheck={false}
                required
                disabled={addSource.isPending}
                aria-invalid={urlMissing}
                aria-describedby={urlMissing ? `${id}-url-error` : undefined}
              />
              {urlMissing && (
                <p
                  id={`${id}-url-error`}
                  role="alert"
                  className="text-xs text-destructive"
                >
                  {t("settings.plugins.sourceUrlRequired")}
                </p>
              )}
            </div>
            <div className="space-y-1.5">
              <label htmlFor={`${id}-branch`} className="text-sm font-medium">
                {t("settings.plugins.sourceBranch")}
              </label>
              <Input
                ref={branchInput}
                id={`${id}-branch`}
                value={branch}
                onChange={(event) => setBranch(event.target.value)}
                autoComplete="off"
                spellCheck={false}
                required
                disabled={addSource.isPending}
                aria-invalid={branchMissing}
                aria-describedby={
                  branchMissing ? `${id}-branch-error` : undefined
                }
              />
              {branchMissing && (
                <p
                  id={`${id}-branch-error`}
                  role="alert"
                  className="text-xs text-destructive"
                >
                  {t("settings.plugins.sourceBranchRequired")}
                </p>
              )}
            </div>
            <label className="flex items-center justify-between gap-3">
              <span className="text-sm font-medium">
                {t("settings.plugins.sourceUseProxy")}
              </span>
              <Switch
                checked={useProxy}
                onCheckedChange={setUseProxy}
                disabled={addSource.isPending}
                aria-label={t("settings.plugins.sourceUseProxy")}
              />
            </label>
            <label className="space-y-1.5">
              <span className="text-sm font-medium">
                {t("settings.plugins.artifactRetrieval")}
              </span>
              <Select
                value={retrievalType}
                onValueChange={(value) =>
                  setRetrievalType(
                    value as MarketplaceArtifactRetrieval["type"],
                  )
                }
                disabled={addSource.isPending}
              >
                <SelectTrigger
                  className="w-full"
                  type="button"
                  aria-label={t("settings.plugins.artifactRetrieval")}
                >
                  <span className="flex-1 text-left">
                    {retrievalType === "direct_https"
                      ? t("settings.plugins.artifactRetrievalDirectHttps")
                      : t("settings.plugins.artifactRetrievalS3SigV4")}
                  </span>
                </SelectTrigger>
                <SelectContent>
                  <SelectItem value="direct_https">
                    {t("settings.plugins.artifactRetrievalDirectHttps")}
                  </SelectItem>
                  <SelectItem value="s3_sigv4">
                    {t("settings.plugins.artifactRetrievalS3SigV4")}
                  </SelectItem>
                </SelectContent>
              </Select>
            </label>
            {s3Selected && (
              <div className="space-y-3 rounded-md border border-border/70 p-3">
                <RetrievalField
                  inputRef={endpointInput}
                  id={`${id}-endpoint`}
                  label={t("settings.plugins.s3Endpoint")}
                  value={endpoint}
                  onChange={setEndpoint}
                  invalid={endpointMissing}
                  disabled={addSource.isPending}
                />
                <RetrievalField
                  inputRef={bucketInput}
                  id={`${id}-bucket`}
                  label={t("settings.plugins.s3Bucket")}
                  value={bucket}
                  onChange={setBucket}
                  invalid={bucketMissing}
                  disabled={addSource.isPending}
                />
                <RetrievalField
                  inputRef={regionInput}
                  id={`${id}-region`}
                  label={t("settings.plugins.s3Region")}
                  value={region}
                  onChange={setRegion}
                  invalid={regionMissing}
                  disabled={addSource.isPending}
                />
                <RetrievalField
                  inputRef={accessKeyInput}
                  id={`${id}-access-key`}
                  label={t("settings.plugins.s3AccessKeyId")}
                  value={accessKeyId}
                  onChange={setAccessKeyId}
                  invalid={accessKeyMissing}
                  disabled={addSource.isPending}
                  autoComplete="off"
                />
                <RetrievalField
                  inputRef={secretKeyInput}
                  id={`${id}-secret-key`}
                  label={t("settings.plugins.s3SecretAccessKey")}
                  value={secretAccessKey}
                  onChange={setSecretAccessKey}
                  invalid={secretKeyMissing}
                  disabled={addSource.isPending}
                  type="password"
                  autoComplete="new-password"
                />
                <p className="text-xs text-muted-foreground">
                  {t("settings.plugins.s3CredentialsRequired")}
                </p>
                {s3Incomplete && (
                  <p
                    id={`${id}-s3-error`}
                    role="alert"
                    className="text-xs text-destructive"
                  >
                    {t("settings.plugins.s3FieldsRequired")}
                  </p>
                )}
              </div>
            )}
            {error !== null && (
              <p role="alert" className="break-words text-sm text-destructive">
                {error}
              </p>
            )}
          </div>
          <DialogFooter className="mx-0 mb-0 shrink-0">
            <Button
              type="button"
              variant="outline"
              disabled={addSource.isPending}
              onClick={() => setOpen(false)}
            >
              {t("common.cancel")}
            </Button>
            <Button type="submit" disabled={addSource.isPending}>
              {addSource.isPending && <IconLoader2 className="animate-spin" />}
              {addSource.isPending
                ? t("settings.plugins.savingSource")
                : t("settings.plugins.addSource")}
            </Button>
          </DialogFooter>
        </form>
      </DialogContent>
    </Dialog>
  );
}

interface RetrievalFieldProps {
  id: string;
  label: string;
  value: string;
  onChange: (value: string) => void;
  invalid: boolean;
  disabled: boolean;
  inputRef: RefObject<HTMLInputElement | null>;
  type?: "password";
  autoComplete?: string;
}

/** One labeled S3 field. Invalid state is announced with the group alert. */
function RetrievalField({
  id,
  label,
  value,
  onChange,
  invalid,
  disabled,
  inputRef,
  type,
  autoComplete,
}: RetrievalFieldProps) {
  return (
    <label className="space-y-1.5" htmlFor={id}>
      <span className="text-sm font-medium">{label}</span>
      <Input
        ref={inputRef}
        id={id}
        type={type}
        value={value}
        onChange={(event) => onChange(event.target.value)}
        aria-label={label}
        aria-invalid={invalid}
        autoComplete={autoComplete}
        spellCheck={false}
        disabled={disabled}
        required
      />
    </label>
  );
}
