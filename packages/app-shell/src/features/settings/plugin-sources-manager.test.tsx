import {
  act,
  fireEvent,
  render,
  screen,
  waitFor,
  within,
} from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { expect, it, vi } from "vitest";
import { RemoteContractError, type ContractsClient } from "@ora/contracts";
import { Dialog, DialogContent, DialogTitle, DialogDescription } from "@ora/ui";
import { AppI18nProvider } from "../../i18n/i18n";
import { appI18n } from "../../i18n/i18n-instance";
import { ContractsClientContext } from "../../contracts-client-context";
import {
  createTestClient,
  type TestHandlers,
} from "../../test/contracts-transport";
import { createPluginMemory, pluginHandlers } from "../../test/memory/plugins";
import { PluginSourcesManager } from "./plugin-sources-manager";

/** State for this test surface; no unrelated domain fixtures are initialized. */
function createFixtureState() {
  return { ...createPluginMemory() };
}

type FixtureState = ReturnType<typeof createFixtureState>;

/** Explicit domain composition for the behaviors exercised by this test file. */
function createFixtureHandlers(state: FixtureState): TestHandlers {
  return {
    ...pluginHandlers(state),
  };
}

// Keep this test worker responsible for initializing the instance used by useTranslation.
void appI18n;

/** Renders the source manager with an isolated query client and contracts client. */
function renderManager(
  client: ContractsClient,
  onBack = vi.fn(),
  outerOpenChange?: (open: boolean) => void,
) {
  const queryClient = new QueryClient({
    defaultOptions: { queries: { retry: false } },
  });
  return render(
    <QueryClientProvider client={queryClient}>
      <ContractsClientContext.Provider value={client}>
        <AppI18nProvider>
          {outerOpenChange ? (
            <Dialog open onOpenChange={outerOpenChange}>
              <DialogContent>
                <DialogTitle>Settings</DialogTitle>
                <DialogDescription>Manage plugins</DialogDescription>
                <PluginSourcesManager onBack={onBack} />
              </DialogContent>
            </Dialog>
          ) : (
            <PluginSourcesManager onBack={onBack} />
          )}
        </AppI18nProvider>
      </ContractsClientContext.Provider>
    </QueryClientProvider>,
  );
}

it("renders configured marketplace sources", async () => {
  const state = createFixtureState();
  state.marketplaceSources.push({
    url: "https://github.com/ora-space/marketplace",
    branch: "main",
    useProxy: false,
    enabled: true,
    artifactRetrieval: { type: "direct_https" },
  });
  const clientHandlers: TestHandlers = createFixtureHandlers(state);
  const client = createTestClient(clientHandlers);

  renderManager(client);

  expect(
    await screen.findByText("https://github.com/ora-space/marketplace"),
  ).toBeInTheDocument();
});

it("adds a marketplace source through the backend", async () => {
  const state = createFixtureState();
  const clientHandlers: TestHandlers = createFixtureHandlers(state);
  const client = createTestClient(clientHandlers);
  const user = userEvent.setup();

  renderManager(client);

  await user.click(screen.getByRole("button", { name: /添加来源|Add source/ }));
  const dialog = await screen.findByRole("dialog", {
    name: /添加来源|Add source/,
  });
  await user.type(
    within(dialog).getByLabelText(/Git URL/),
    "https://github.com/example/marketplace",
  );
  await user.click(
    within(dialog).getByRole("button", { name: /添加来源|Add source/ }),
  );

  await waitFor(() =>
    expect(state.marketplaceSources).toEqual([
      {
        url: "https://github.com/example/marketplace",
        branch: "main",
        useProxy: false,
        enabled: true,
        artifactRetrieval: { type: "direct_https" },
      },
    ]),
  );
  await waitFor(() =>
    expect(screen.queryByRole("dialog")).not.toBeInTheDocument(),
  );
  expect(
    await screen.findByText("https://github.com/example/marketplace"),
  ).toBeInTheDocument();
});

it("removes a marketplace source through the backend", async () => {
  const state = createFixtureState();
  state.marketplaceSources.push({
    url: "https://github.com/ora-space/marketplace",
    branch: "main",
    useProxy: false,
    enabled: true,
    artifactRetrieval: { type: "direct_https" },
  });
  const clientHandlers: TestHandlers = createFixtureHandlers(state);
  const client = createTestClient(clientHandlers);
  const deleteSource = vi.spyOn(client.plugin, "deleteSource");

  renderManager(client);

  const deleteButton = await screen.findByRole("button", {
    name: /删除|Delete/,
  });
  expect(deleteButton).toBeEnabled();
  fireEvent.click(deleteButton);

  await waitFor(() =>
    expect(deleteSource).toHaveBeenCalledWith({
      url: "https://github.com/ora-space/marketplace",
    }),
  );
  await waitFor(() => expect(state.marketplaceSources).toEqual([]));
});

it("edits a marketplace source URL and branch", async () => {
  const state = createFixtureState();
  state.marketplaceSources.push({
    url: "https://github.com/ora-space/marketplace",
    branch: "main",
    useProxy: false,
    enabled: true,
    artifactRetrieval: { type: "direct_https" },
  });
  const clientHandlers: TestHandlers = createFixtureHandlers(state);
  const client = createTestClient(clientHandlers);
  const user = userEvent.setup();

  renderManager(client);

  await user.click(await screen.findByRole("button", { name: /编辑|Edit/ }));
  const dialog = await screen.findByRole("dialog");
  const urlInput = within(dialog).getByLabelText(/Git URL/);
  const branchInput = within(dialog).getByLabelText(/分支|Branch/);
  await user.clear(urlInput);
  await user.type(urlInput, "https://github.com/example/marketplace");
  await user.clear(branchInput);
  await user.type(branchInput, "release");
  await user.click(within(dialog).getByRole("button", { name: /保存|Save/ }));

  await waitFor(() =>
    expect(state.marketplaceSources).toEqual([
      {
        url: "https://github.com/example/marketplace",
        branch: "release",
        useProxy: false,
        enabled: true,
        artifactRetrieval: { type: "direct_https" },
      },
    ]),
  );
});

it("configures S3 SigV4 retrieval without returning credentials", async () => {
  const state = createFixtureState();
  state.marketplaceSources.push({
    url: "https://github.com/ora-space/marketplace",
    branch: "main",
    useProxy: false,
    enabled: true,
    artifactRetrieval: { type: "direct_https" },
  });
  const clientHandlers: TestHandlers = createFixtureHandlers(state);
  const client = createTestClient(clientHandlers);
  const updateSource = vi.spyOn(client.plugin, "updateSource");
  const user = userEvent.setup();

  renderManager(client);

  await user.click(await screen.findByRole("button", { name: /编辑|Edit/ }));
  const dialog = await screen.findByRole("dialog");
  await user.click(
    within(dialog).getByLabelText(/插件包获取方式|Plugin package retrieval/),
  );
  await user.click(await screen.findByRole("option", { name: /S3.*SigV4/i }));
  await user.type(
    within(dialog).getByLabelText(/S3 Endpoint/i),
    "https://s3.example.com",
  );
  await user.type(within(dialog).getByLabelText(/^Bucket$/i), "plugins");
  await user.type(within(dialog).getByLabelText(/^Region$/i), "region-1");
  await user.type(within(dialog).getByLabelText(/Access Key ID/i), "access");
  await user.type(
    within(dialog).getByLabelText(/Secret Access Key/i),
    "secret",
  );
  await user.click(within(dialog).getByRole("button", { name: /保存|Save/ }));

  await waitFor(() =>
    expect(updateSource).toHaveBeenCalledWith({
      url: "https://github.com/ora-space/marketplace",
      newUrl: "https://github.com/ora-space/marketplace",
      branch: "main",
      useProxy: false,
      enabled: true,
      artifactRetrieval: {
        type: "s3_sigv4",
        endpoint: "https://s3.example.com",
        bucket: "plugins",
        region: "region-1",
        credentials: {
          action: "replace",
          accessKeyId: "access",
          secretAccessKey: "secret",
        },
      },
    }),
  );
  await waitFor(() =>
    expect(state.marketplaceSources[0]?.artifactRetrieval).toEqual({
      type: "s3_sigv4",
      endpoint: "https://s3.example.com",
      bucket: "plugins",
      region: "region-1",
    }),
  );
});

it("disables a marketplace source without removing it", async () => {
  const state = createFixtureState();
  state.marketplaceSources.push({
    url: "https://github.com/ora-space/marketplace",
    branch: "main",
    useProxy: false,
    enabled: true,
    artifactRetrieval: { type: "direct_https" },
  });
  const clientHandlers: TestHandlers = createFixtureHandlers(state);
  const client = createTestClient(clientHandlers);
  const user = userEvent.setup();

  renderManager(client);

  await user.click(await screen.findByRole("button", { name: /禁用|Disable/ }));

  await waitFor(() =>
    expect(state.marketplaceSources).toEqual([
      {
        url: "https://github.com/ora-space/marketplace",
        branch: "main",
        useProxy: false,
        enabled: false,
        artifactRetrieval: { type: "direct_https" },
      },
    ]),
  );
});

it("opens a source dialog when Add source is clicked before entering a URL", async () => {
  const state = createFixtureState();
  const user = userEvent.setup();
  renderManager(createTestClient(createFixtureHandlers(state)));

  await user.click(screen.getByRole("button", { name: /添加来源|Add source/ }));

  const dialog = await screen.findByRole("dialog", {
    name: /添加来源|Add source/,
  });
  expect(within(dialog).getByLabelText(/Git URL/)).toHaveFocus();
  expect(within(dialog).getByLabelText(/分支|Branch/)).toHaveValue("main");
  expect(state.marketplaceSources).toEqual([]);
});

it("shows required-field errors, then submits the trimmed branch and proxy choice with Enter", async () => {
  const state = createFixtureState();
  const handlers = createFixtureHandlers(state);
  const add = vi.fn(handlers.addMarketplaceSource!);
  handlers.addMarketplaceSource = add;
  const user = userEvent.setup();
  renderManager(createTestClient(handlers));
  await user.click(screen.getByRole("button", { name: /添加来源|Add source/ }));
  const dialog = await screen.findByRole("dialog");
  const url = within(dialog).getByLabelText(/Git URL/);
  const branch = within(dialog).getByLabelText(/分支|Branch/);
  const submit = within(dialog).getByRole("button", {
    name: /添加来源|Add source/,
  });
  await user.click(submit);
  expect(url).toHaveAttribute("aria-invalid", "true");
  expect(within(dialog).getByRole("alert")).toHaveTextContent(/Git URL/);
  expect(url).toHaveFocus();
  expect(add).not.toHaveBeenCalled();
  await user.type(url, " https://github.com/example/marketplace ");
  await user.clear(branch);
  await user.click(submit);
  expect(branch).toHaveAttribute("aria-invalid", "true");
  expect(branch).toHaveFocus();
  expect(add).not.toHaveBeenCalled();
  await user.type(branch, " release ");
  await user.click(
    within(dialog).getByRole("switch", { name: /使用代理|Use proxy/ }),
  );
  await user.click(branch);
  await user.keyboard("{Enter}");
  await waitFor(() =>
    expect(state.marketplaceSources).toEqual([
      {
        url: "https://github.com/example/marketplace",
        branch: "release",
        useProxy: true,
        enabled: true,
        artifactRetrieval: { type: "direct_https" },
      },
    ]),
  );
  expect(add).toHaveBeenCalledTimes(1);
});

it("cancels drafts and Escape closes only the nested source dialog, restoring focus", async () => {
  const state = createFixtureState();
  const outerOpenChange = vi.fn();
  const user = userEvent.setup();
  renderManager(
    createTestClient(createFixtureHandlers(state)),
    vi.fn(),
    outerOpenChange,
  );
  const trigger = screen.getByRole("button", { name: /添加来源|Add source/ });
  await user.click(trigger);
  let dialog = await screen.findByRole("dialog", {
    name: /添加来源|Add source/,
  });
  await user.type(
    within(dialog).getByLabelText(/Git URL/),
    "https://example.com/draft",
  );
  await user.click(within(dialog).getByRole("button", { name: /取消|Cancel/ }));
  await waitFor(() => expect(trigger).toHaveFocus());
  await user.click(trigger);
  dialog = await screen.findByRole("dialog", { name: /添加来源|Add source/ });
  expect(within(dialog).getByLabelText(/Git URL/)).toHaveValue("");
  expect(within(dialog).getByLabelText(/分支|Branch/)).toHaveValue("main");
  await user.keyboard("{Escape}");
  await waitFor(() => expect(trigger).toHaveFocus());
  expect(screen.getByRole("dialog", { name: "Settings" })).toBeInTheDocument();
  expect(outerOpenChange).not.toHaveBeenCalled();
  expect(state.marketplaceSources).toEqual([]);
});

it("keeps the dialog open during submission and prevents repeated requests", async () => {
  const state = createFixtureState();
  const handlers = createFixtureHandlers(state);
  const persist = handlers.addMarketplaceSource!;
  let finish!: () => void;
  const gate = new Promise<void>((resolve) => {
    finish = resolve;
  });
  const add = vi.fn<NonNullable<TestHandlers["addMarketplaceSource"]>>(
    async (request) => {
      await gate;
      return persist(request);
    },
  );
  handlers.addMarketplaceSource = add;
  const user = userEvent.setup();
  renderManager(createTestClient(handlers));
  const trigger = screen.getByRole("button", { name: /添加来源|Add source/ });
  await user.click(trigger);
  const dialog = await screen.findByRole("dialog");
  await user.type(
    within(dialog).getByLabelText(/Git URL/),
    "https://example.com/marketplace",
  );
  await user.dblClick(
    within(dialog).getByRole("button", { name: /添加来源|Add source/ }),
  );
  expect(
    within(dialog).getByRole("button", { name: /保存中|Saving/ }),
  ).toBeDisabled();
  expect(
    within(dialog).getByRole("button", { name: /取消|Cancel/ }),
  ).toBeDisabled();
  expect(within(dialog).getByLabelText(/Git URL/)).toBeDisabled();
  await user.keyboard("{Escape}");
  expect(dialog).toBeInTheDocument();
  expect(add).toHaveBeenCalledTimes(1);
  await act(async () => {
    finish();
  });
  await waitFor(() =>
    expect(screen.queryByRole("dialog")).not.toBeInTheDocument(),
  );
  await waitFor(() => expect(trigger).toHaveFocus());
});

it("preserves input and shows a localized backend error before retrying", async () => {
  const state = createFixtureState();
  const handlers = createFixtureHandlers(state);
  const persist = handlers.addMarketplaceSource!;
  const add = vi.fn(persist).mockRejectedValueOnce(
    new RemoteContractError(
      {
        code: "internal_error",
        params: {},
        requestId: "source-add-test",
      },
      null,
    ),
  );
  handlers.addMarketplaceSource = add;
  const user = userEvent.setup();
  renderManager(createTestClient(handlers));
  await user.click(screen.getByRole("button", { name: /添加来源|Add source/ }));
  const dialog = await screen.findByRole("dialog");
  await user.type(
    within(dialog).getByLabelText(/Git URL/),
    "https://example.com/marketplace",
  );
  await user.click(
    within(dialog).getByRole("button", { name: /添加来源|Add source/ }),
  );
  expect(await within(dialog).findByRole("alert")).toHaveTextContent(
    "source-add-test",
  );
  expect(within(dialog).getByLabelText(/Git URL/)).toHaveValue(
    "https://example.com/marketplace",
  );
  expect(state.marketplaceSources).toEqual([]);
  await user.click(
    within(dialog).getByRole("button", { name: /添加来源|Add source/ }),
  );
  await waitFor(() =>
    expect(screen.queryByRole("dialog")).not.toBeInTheDocument(),
  );
  expect(add).toHaveBeenCalledTimes(2);
  expect(state.marketplaceSources).toHaveLength(1);
});

it("keeps Direct HTTPS as the default and hides S3 fields until that method is chosen", async () => {
  const state = createFixtureState();
  const user = userEvent.setup();
  renderManager(createTestClient(createFixtureHandlers(state)));
  await user.click(screen.getByRole("button", { name: /添加来源|Add source/ }));
  const dialog = await screen.findByRole("dialog", {
    name: /添加来源|Add source/,
  });
  expect(
    within(dialog).getByLabelText(/插件包获取方式|Plugin package retrieval/),
  ).toHaveTextContent(/HTTPS 直接获取|Direct HTTPS/);
  expect(
    within(dialog).queryByLabelText(/S3 Endpoint/i),
  ).not.toBeInTheDocument();
});

it("requires the S3 fields before creating a source", async () => {
  const state = createFixtureState();
  const handlers = createFixtureHandlers(state);
  const add = vi.fn(handlers.addMarketplaceSource!);
  handlers.addMarketplaceSource = add;
  const user = userEvent.setup();
  renderManager(createTestClient(handlers));
  await user.click(screen.getByRole("button", { name: /添加来源|Add source/ }));
  const dialog = await screen.findByRole("dialog");
  await user.type(
    within(dialog).getByLabelText(/Git URL/),
    "https://example.com/marketplace",
  );
  await user.click(
    within(dialog).getByLabelText(/插件包获取方式|Plugin package retrieval/),
  );
  await user.click(await screen.findByRole("option", { name: /S3.*SigV4/i }));
  await user.click(
    within(dialog).getByRole("button", { name: /添加来源|Add source/ }),
  );
  expect(within(dialog).getByLabelText(/S3 Endpoint/i)).toHaveAttribute(
    "aria-invalid",
    "true",
  );
  expect(within(dialog).getByRole("alert")).toHaveTextContent(/S3 [Ee]ndpoint/);
  expect(within(dialog).getByLabelText(/S3 Endpoint/i)).toHaveFocus();
  expect(add).not.toHaveBeenCalled();
  expect(state.marketplaceSources).toEqual([]);
});

it("stores S3 retrieval with the new source and omits credentials from the list", async () => {
  const state = createFixtureState();
  const handlers = createFixtureHandlers(state);
  const update = vi.fn(handlers.updateMarketplaceSource!);
  handlers.updateMarketplaceSource = update;
  const user = userEvent.setup();
  renderManager(createTestClient(handlers));
  await user.click(screen.getByRole("button", { name: /添加来源|Add source/ }));
  const dialog = await screen.findByRole("dialog");
  await user.type(
    within(dialog).getByLabelText(/Git URL/),
    "https://example.com/marketplace",
  );
  await user.click(
    within(dialog).getByLabelText(/插件包获取方式|Plugin package retrieval/),
  );
  await user.click(await screen.findByRole("option", { name: /S3.*SigV4/i }));
  await user.type(
    within(dialog).getByLabelText(/S3 Endpoint/i),
    "https://s3.example.com",
  );
  await user.type(within(dialog).getByLabelText(/^Bucket$/i), "plugins");
  await user.type(within(dialog).getByLabelText(/^Region$/i), "region-1");
  await user.type(within(dialog).getByLabelText(/Access Key ID/i), "access");
  await user.type(
    within(dialog).getByLabelText(/Secret Access Key/i),
    "secret",
  );
  await user.click(
    within(dialog).getByRole("button", { name: /添加来源|Add source/ }),
  );
  await waitFor(() =>
    expect(state.marketplaceSources).toEqual([
      {
        url: "https://example.com/marketplace",
        branch: "main",
        useProxy: false,
        enabled: true,
        artifactRetrieval: {
          type: "s3_sigv4",
          endpoint: "https://s3.example.com",
          bucket: "plugins",
          region: "region-1",
        },
      },
    ]),
  );
  expect(update.mock.calls[0]?.[0]).toEqual({
    url: "https://example.com/marketplace",
    newUrl: "https://example.com/marketplace",
    branch: "main",
    useProxy: false,
    enabled: true,
    artifactRetrieval: {
      type: "s3_sigv4",
      endpoint: "https://s3.example.com",
      bucket: "plugins",
      region: "region-1",
      credentials: {
        action: "replace",
        accessKeyId: "access",
        secretAccessKey: "secret",
      },
    },
  });
  await waitFor(() =>
    expect(screen.queryByRole("dialog")).not.toBeInTheDocument(),
  );
});

it("removes the new source when the S3 credential write fails", async () => {
  const state = createFixtureState();
  const handlers = createFixtureHandlers(state);
  handlers.updateMarketplaceSource = vi.fn().mockRejectedValue(
    new RemoteContractError(
      {
        code: "internal_error",
        params: {},
        requestId: "source-add-s3",
      },
      null,
    ),
  );
  const remove = vi.fn(handlers.deleteMarketplaceSource!);
  handlers.deleteMarketplaceSource = remove;
  const user = userEvent.setup();
  renderManager(createTestClient(handlers));
  await user.click(screen.getByRole("button", { name: /添加来源|Add source/ }));
  const dialog = await screen.findByRole("dialog");
  await user.type(
    within(dialog).getByLabelText(/Git URL/),
    "https://example.com/marketplace",
  );
  await user.click(
    within(dialog).getByLabelText(/插件包获取方式|Plugin package retrieval/),
  );
  await user.click(await screen.findByRole("option", { name: /S3.*SigV4/i }));
  await user.type(
    within(dialog).getByLabelText(/S3 Endpoint/i),
    "https://s3.example.com",
  );
  await user.type(within(dialog).getByLabelText(/^Bucket$/i), "plugins");
  await user.type(within(dialog).getByLabelText(/^Region$/i), "region-1");
  await user.type(within(dialog).getByLabelText(/Access Key ID/i), "access");
  await user.type(
    within(dialog).getByLabelText(/Secret Access Key/i),
    "secret",
  );
  await user.click(
    within(dialog).getByRole("button", { name: /添加来源|Add source/ }),
  );
  expect(await within(dialog).findByRole("alert")).toHaveTextContent(
    "source-add-s3",
  );
  expect(remove.mock.calls[0]?.[0]).toEqual({
    url: "https://example.com/marketplace",
  });
  expect(state.marketplaceSources).toEqual([]);
  expect(within(dialog).getByLabelText(/Git URL/)).toHaveValue(
    "https://example.com/marketplace",
  );
  expect(within(dialog).getByLabelText(/Secret Access Key/i)).toHaveValue(
    "secret",
  );
});
