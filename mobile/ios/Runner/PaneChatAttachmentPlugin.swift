import Flutter
import UIKit
import UniformTypeIdentifiers

private let chatAttachmentMaxBytes = 10 * 1024 * 1024
private let chatAttachmentMaxFiles = 10
private let chatAttachmentReadBufferBytes = 8 * 1024

final class PaneChatAttachmentPlugin: NSObject, FlutterPlugin, UIDropInteractionDelegate {
  private let channel: FlutterMethodChannel
  private weak var registrar: FlutterPluginRegistrar?
  private weak var dropView: UIView?
  private var dropInteraction: UIDropInteraction?
  private var enabled = false
  private var epoch = 0

  private init(channel: FlutterMethodChannel, registrar: FlutterPluginRegistrar) {
    self.channel = channel
    self.registrar = registrar
    super.init()
  }

  static func register(with registrar: FlutterPluginRegistrar) {
    let channel = FlutterMethodChannel(
      name: "kanvibe/pane-chat-attachments", binaryMessenger: registrar.messenger())
    let instance = PaneChatAttachmentPlugin(channel: channel, registrar: registrar)
    registrar.addMethodCallDelegate(instance, channel: channel)
  }

  func handle(_ call: FlutterMethodCall, result: @escaping FlutterResult) {
    let arguments = call.arguments as? [String: Any] ?? [:]
    switch call.method {
    case "setDropEnabled":
      enabled = arguments["enabled"] as? Bool == true
      epoch = arguments["epoch"] as? Int ?? epoch + 1
      updateDropInteraction()
      result(nil)
    case "readClipboardImage":
      guard enabled, arguments["epoch"] as? Int == epoch else {
        result(FlutterError(code: "inactive", message: "첨부 대상이 변경되었습니다.", details: nil))
        return
      }
      let requestEpoch = epoch
      guard let provider = UIPasteboard.general.itemProviders.first(where: {
        $0.hasItemConformingToTypeIdentifier(UTType.image.identifier)
      }) else {
        result(nil)
        return
      }
      loadAttachment(provider, imageOnly: true) { [weak self] loaded in
        DispatchQueue.main.async {
          guard let self = self, self.enabled, self.epoch == requestEpoch else {
            result(FlutterError(code: "inactive", message: "첨부 대상이 변경되었습니다.", details: nil))
            return
          }
          switch loaded {
          case .success(let file): result(file)
          case .failure(let error):
            result(FlutterError(code: "attachment_read_failed", message: attachmentErrorMessage(error), details: nil))
          }
        }
      }
    default:
      result(FlutterMethodNotImplemented)
    }
  }

  private func updateDropInteraction() {
    guard enabled, let view = registrar?.viewController?.view else {
      if let interaction = dropInteraction { dropView?.removeInteraction(interaction) }
      dropInteraction = nil
      dropView = nil
      return
    }
    if dropView === view { return }
    if let interaction = dropInteraction { dropView?.removeInteraction(interaction) }
    let interaction = UIDropInteraction(delegate: self)
    view.addInteraction(interaction)
    dropInteraction = interaction
    dropView = view
  }

  func dropInteraction(_ interaction: UIDropInteraction, canHandle session: UIDropSession) -> Bool {
    enabled && !session.items.isEmpty && session.items.allSatisfy { item in
      item.itemProvider.hasItemConformingToTypeIdentifier(UTType.image.identifier)
        || item.itemProvider.hasItemConformingToTypeIdentifier(UTType.fileURL.identifier)
        || item.itemProvider.suggestedName?.isEmpty == false
    }
  }

  func dropInteraction(_ interaction: UIDropInteraction, sessionDidUpdate session: UIDropSession) -> UIDropProposal {
    UIDropProposal(operation: enabled ? .copy : .cancel)
  }

  func dropInteraction(_ interaction: UIDropInteraction, performDrop session: UIDropSession) {
    guard enabled else { return }
    let requestEpoch = epoch
    guard session.items.count <= chatAttachmentMaxFiles else {
      reportDropFailure(requestEpoch, "첨부는 한 번에 10개까지 추가할 수 있습니다.")
      return
    }
    loadDroppedItems(session.items.map(\.itemProvider), at: 0, files: [], requestEpoch: requestEpoch)
  }

  private func loadDroppedItems(
    _ providers: [NSItemProvider], at index: Int, files: [[String: Any]], requestEpoch: Int
  ) {
    guard enabled, epoch == requestEpoch else { return }
    if index == providers.count {
      channel.invokeMethod("filesDropped", arguments: ["epoch": requestEpoch, "files": files])
      return
    }
    loadAttachment(providers[index], imageOnly: false) { [weak self] loaded in
      DispatchQueue.main.async {
        guard let self = self, self.enabled, self.epoch == requestEpoch else { return }
        switch loaded {
        case .success(let file):
          self.loadDroppedItems(providers, at: index + 1, files: files + [file], requestEpoch: requestEpoch)
        case .failure(let error): self.reportDropFailure(requestEpoch, attachmentErrorMessage(error))
        }
      }
    }
  }

  private func reportDropFailure(_ requestEpoch: Int, _ message: String) {
    guard enabled, epoch == requestEpoch else { return }
    channel.invokeMethod("dropFailed", arguments: ["epoch": requestEpoch, "message": message])
  }

  func detachFromEngine(for registrar: FlutterPluginRegistrar) {
    enabled = false
    epoch += 1
    updateDropInteraction()
    channel.setMethodCallHandler(nil)
  }
}

private struct ChatAttachmentReadError: Error {
  let message: String
}

private func attachmentErrorMessage(_ error: Error) -> String {
  (error as? ChatAttachmentReadError)?.message ?? "선택한 파일을 읽지 못했습니다. 다시 시도해 주세요."
}

private func loadAttachment(
  _ provider: NSItemProvider,
  imageOnly: Bool,
  completion: @escaping (Result<[String: Any], Error>) -> Void
) {
  let identifier = provider.registeredTypeIdentifiers.first { identifier in
    guard let type = UTType(identifier), type != .fileURL else { return false }
    return type.conforms(to: imageOnly ? .image : .data)
  }
  if let identifier = identifier {
    provider.loadFileRepresentation(forTypeIdentifier: identifier) { url, error in
      do {
        guard let url = url else { throw error ?? ChatAttachmentReadError(message: "파일을 열지 못했습니다.") }
        completion(.success(try readAttachmentFile(url, name: provider.suggestedName, type: UTType(identifier))))
      } catch { completion(.failure(error)) }
    }
  } else if !imageOnly && provider.hasItemConformingToTypeIdentifier(UTType.fileURL.identifier) {
    provider.loadItem(forTypeIdentifier: UTType.fileURL.identifier, options: nil) { item, error in
      do {
        guard let url = item as? URL else { throw error ?? ChatAttachmentReadError(message: "파일 주소를 읽지 못했습니다.") }
        completion(.success(try readAttachmentFile(url, name: provider.suggestedName, type: nil)))
      } catch { completion(.failure(error)) }
    }
  } else {
    completion(.failure(ChatAttachmentReadError(message: "선택한 파일 형식을 읽지 못했습니다.")))
  }
}

private func readAttachmentFile(_ url: URL, name: String?, type: UTType?) throws -> [String: Any] {
  guard url.isFileURL else { throw ChatAttachmentReadError(message: "선택한 파일 주소를 읽을 수 없습니다.") }
  let scoped = url.startAccessingSecurityScopedResource()
  defer { if scoped { url.stopAccessingSecurityScopedResource() } }
  if let size = try url.resourceValues(forKeys: [.fileSizeKey]).fileSize, size > chatAttachmentMaxBytes {
    throw ChatAttachmentReadError(message: "파일은 10 MiB 이하여야 합니다.")
  }
  guard let stream = InputStream(url: url) else { throw ChatAttachmentReadError(message: "파일을 열지 못했습니다.") }
  stream.open()
  defer { stream.close() }
  var data = Data()
  var buffer = [UInt8](repeating: 0, count: chatAttachmentReadBufferBytes)
  while true {
    let count = stream.read(&buffer, maxLength: buffer.count)
    if count < 0 { throw stream.streamError ?? ChatAttachmentReadError(message: "파일을 읽지 못했습니다.") }
    if count == 0 { break }
    guard data.count + count <= chatAttachmentMaxBytes else {
      throw ChatAttachmentReadError(message: "파일은 10 MiB 이하여야 합니다.")
    }
    data.append(buffer, count: count)
  }
  let mimeType = type?.preferredMIMEType ?? UTType(filenameExtension: url.pathExtension)?.preferredMIMEType ?? "application/octet-stream"
  return ["name": name?.isEmpty == false ? name! : url.lastPathComponent,
          "mimeType": mimeType, "bytes": FlutterStandardTypedData(bytes: data)]
}
