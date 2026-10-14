import { useId, useRef, useState } from "react";
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
  Switch,
  toast,
} from "@ora/ui";
import { IconLoader2, IconPlus } from "@tabler/icons-react";
import { localizeContractError } from "../../i18n/contract-error";
import { useContractErrorToast } from "../../i18n/use-contract-error-toast";
import { useAddMarketplaceSource } from "../../state/hooks/use-marketplace-sources";

/** Owns the new-source draft while the existing mutation owns persistence and invalidation. */
export function PluginSourceAddDialog() {
  const { t } = useTranslation();
  const showContractError = useContractErrorToast();
  const addSource = useAddMarketplaceSource();
  const [open, setOpen] = useState(false);
  const [url, setUrl] = useState("");
  const [branch, setBranch] = useState("main");
  const [useProxy, setUseProxy] = useState(false);
  const [submitted, setSubmitted] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const urlInput = useRef<HTMLInputElement>(null);
  const branchInput = useRef<HTMLInputElement>(null);
  const submitting = useRef(false);
  const id = useId();
  const urlMissing = submitted && url.trim() === "";
  const branchMissing = submitted && branch.trim() === "";

  return (
    <Dialog
      open={open}
      onOpenChange={(nextOpen, details) => {
        if (submitting.current) {
          details.cancel();
          return;
        }
        if (nextOpen) {
          setUrl("");
          setBranch("main");
          setUseProxy(false);
          setSubmitted(false);
          setError(null);
          addSource.reset();
        }
        setOpen(nextOpen);
      }}
    >
      <DialogTrigger render={<Button size="sm" />}>
        <IconPlus />
        {t("settings.plugins.addSource")}
      </DialogTrigger>
      <DialogContent
        className="max-h-[calc(100dvh-2rem)] overflow-y-auto sm:max-w-md"
        initialFocus={urlInput}
        showCloseButton={!addSource.isPending}
      >
        <form
          className="space-y-4"
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
            submitting.current = true;
            addSource.mutate(
              { url: url.trim(), branch: branch.trim(), useProxy },
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
          <DialogHeader>
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
          {error !== null && (
            <p role="alert" className="break-words text-sm text-destructive">
              {error}
            </p>
          )}
          <DialogFooter>
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
